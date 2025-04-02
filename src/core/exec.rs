use std::{collections::HashMap, str::FromStr, sync::LazyLock};

use alloy::{
    primitives::{Address, B256},
    providers::{Provider, ProviderBuilder, RootProvider},
    rpc::types::{
        simulate::{SimBlock, SimulatePayload},
        TransactionInput, TransactionRequest,
    },
    signers::local::PrivateKeySigner,
    sol_types::SolValue,
    transports::http::Http,
};
use alloy_chains::NamedChain;
use num_bigint::BigUint;
use reqwest::Client;
use tycho_execution::encoding::{
    evm::encoder_builder::EVMEncoderBuilder,
    models::{Solution, Transaction},
    tycho_encoder::TychoEncoder,
};

use alloy_primitives::{Bytes as AlloyBytes, U256};

use crate::{
    data::fmt::SrzProtocolComponent,
    types::{self, ChainSimu, EnvConfig, ExecutionPayload, ExecutionRequest, Network, SrzTransactionRequest},
    utils::r#static::execution,
};

static ROUTER_ADDRESSES: LazyLock<HashMap<ChainSimu, tycho_simulation::tycho_core::Bytes>> = LazyLock::new(|| {
    HashMap::from([
        (
            ChainSimu::Ethereum,
            "0x023eea66B260FA2E109B0764774837629cC41FeF"
                .parse::<tycho_simulation::tycho_core::Bytes>()
                .expect("Failed to create Ethereum router address"),
        ),
        (
            ChainSimu::Base,
            "0x94ebf984511b06bab48545495b754760bfaa566e"
                .parse::<tycho_simulation::tycho_core::Bytes>()
                .expect("Failed to create Base router address"),
        ),
    ])
});

/// Build 2 transactions for the given solution:
/// 1. Approve the given token to the router address.
/// 2. Swap the given token for the checked token using the router address.
/// The transactions are built using the given network and nonce + 1 on the 2nd transaction.
pub fn batch(network: Network, solution: Solution, encoded: Transaction, block: alloy::rpc::types::Block, nonce: u64) -> Option<(TransactionRequest, TransactionRequest)> {
    tracing::debug!("Block: {:?}", block);
    let base_fee = block.header.base_fee_per_gas.expect("Base fee not available");
    let max_priority_fee_per_gas = 1_000_000_000u128; // 1 Gwei, not suited for L2s.
    let max_fee_per_gas = base_fee + max_priority_fee_per_gas;
    tracing::debug!("Nonce: {}", nonce);
    // --- Approve Tx with Permit2 ---
    let amount: u128 = solution.given_amount.clone().to_string().parse().expect("Couldn't convert given_amount to u128"); // ?
    let args = (Address::from_str(&network.permit2).expect("Couldn't convert to address"), amount);
    let data = tycho_execution::encoding::evm::utils::encode_input(execution::APPROVE_FN_SIGNATURE, args.abi_encode());
    let sender = solution.sender.clone().to_string().parse().expect("Failed to parse sender");
    let approval = TransactionRequest {
        to: Some(alloy::primitives::TxKind::Call(solution.given_token.clone().to_string().parse().expect("Failed to parse given_token"))),
        from: Some(sender),
        value: None,
        input: TransactionInput {
            input: Some(AlloyBytes::from(data)),
            data: None,
        },
        gas: Some(execution::DEFAULT_APPROVE_GAS),
        chain_id: Some(network.chainid),
        max_fee_per_gas: Some(max_fee_per_gas),
        max_priority_fee_per_gas: Some(max_priority_fee_per_gas),
        nonce: Some(nonce),
        ..Default::default()
    };
    // --- Swap Tx ---
    let swap = TransactionRequest {
        to: Some(alloy_primitives::TxKind::Call(Address::from_slice(&encoded.to))),
        from: Some(sender),
        value: Some(U256::from(0)),
        input: TransactionInput {
            input: Some(AlloyBytes::from(encoded.data)),
            data: None,
        },
        gas: Some(1_000_000u128),
        chain_id: Some(network.chainid),
        max_fee_per_gas: Some(max_fee_per_gas),
        max_priority_fee_per_gas: Some(max_priority_fee_per_gas),
        nonce: Some(nonce + 1),
        ..Default::default()
    };
    Some((approval, swap))
}

/// Build a swap solution Tycho structure
pub async fn solution(chain: ChainSimu, request: ExecutionRequest) -> Option<Solution> {
    tracing::debug!("Preparing swap. Request: {:?}", request);
    let router = ROUTER_ADDRESSES.get(&chain).expect("Router address not found").clone();
    let sum = request.distribution.iter().fold(0., |acc, x| acc + x);
    if !(99. ..=101.).contains(&sum) {
        tracing::debug!("Invalid distribution: {:?}, sum = {}", request.distribution, sum);
        return None;
    }
    let mut swaps = vec![];
    for (x, dist) in request.distribution.iter().enumerate() {
        if *dist > 0. {
            let cp = request.components[x].clone();
            let original = SrzProtocolComponent::original(cp.clone(), chain);
            let input = tycho_simulation::tycho_core::Bytes::from_str(request.input.clone().address.to_lowercase().as_str()).unwrap();
            let output = tycho_simulation::tycho_core::Bytes::from_str(request.output.clone().address.to_lowercase().as_str()).unwrap();
            swaps.push(tycho_execution::encoding::models::Swap::new(original.clone(), input, output, dist / 100.));
        }
    }
    let amount_in = (request.amount * 10f64.powi(request.input.decimals as i32)) as u128;
    let amount_in = BigUint::from(amount_in);
    let expected_amount_out = match request.expected_amount_out {
        Some(amount) => {
            let amount = (amount * 10f64.powi(request.output.decimals as i32)) as u128;
            Some(BigUint::from(amount))
        }
        None => None,
    };
    let solution: Solution = Solution {
        // Addresses
        sender: tycho_simulation::tycho_core::Bytes::from_str(request.sender.to_lowercase().as_str()).unwrap(),
        receiver: tycho_simulation::tycho_core::Bytes::from_str(request.sender.to_lowercase().as_str()).unwrap(),
        given_token: tycho_simulation::tycho_core::Bytes::from_str(request.input.clone().address.to_lowercase().as_str()).unwrap(),
        checked_token: tycho_simulation::tycho_core::Bytes::from_str(request.output.clone().address.to_lowercase().as_str()).unwrap(),
        // Others fields
        given_amount: amount_in.clone(),
        slippage: Some(execution::EXEC_DEFAULT_SLIPPAGE),
        expected_amount: expected_amount_out,
        exact_out: false,     // It's an exact in solution  Currently only exact input solutions are supported.
        checked_amount: None, // The amount out will not be checked in execution
        swaps: swaps.clone(),
        router_address: router,
        ..Default::default()
    };
    tracing::debug!("Solution: {:?}", solution);
    Some(solution)
}

/// Build swap transactions on the specified network for the given request.
pub async fn build(network: Network, request: ExecutionRequest) -> Result<ExecutionPayload, String> {
    let (_, _, chain) = types::chain(network.name.clone()).unwrap();
    let nchain = match network.name.as_str() {
        "ethereum" => NamedChain::Mainnet,
        "base" => NamedChain::Base,
        "arbitrum" => NamedChain::Arbitrum,
        _ => {
            tracing::error!("Unsupported network: {}", network.name);
            return Err("Unsupported network".to_string());
        }
    };
    let tokens = vec![request.input.clone().address, request.output.clone().address];
    let provider = ProviderBuilder::new().with_chain(nchain).on_http(network.rpc.parse().expect("Failed to parse RPC_URL"));
    match super::rpc::erc20b(&provider, request.sender.clone(), tokens.clone()).await {
        Ok(balances) => {
            tracing::debug!("Building swap calldata and transactions ...");
            tracing::debug!("Balances of sender {} => input token: {} and output tokens {}", request.sender, balances[0], balances[1]);
            if let Some(solution) = solution(chain, request.clone()).await {
                let header: alloy::rpc::types::Block = provider.get_block_by_number(alloy::eips::BlockNumberOrTag::Latest, false).await.unwrap().unwrap();
                let nonce = provider.get_transaction_count(solution.sender.to_string().parse().unwrap()).await.unwrap();
                match EVMEncoderBuilder::new().chain(chain).build() {
                    Ok(encoder) => {
                        // let mut encoder = encoder;
                        // encoder.set_chain_id(network.chainid);
                        // encoder.set_gas_limit(execution::DEFAULT_APPROVE_GAS);
                        // encoder.set_gas_price(U256::from(1_000_000_000u128));
                        // encoder.set_nonce(nonce);
                        // encoder.set_sender(solution.sender.clone());
                        // encoder.set_receiver(solution.router_address.clone());
                        // encoder.set_value(U256::from(0));
                        // encoder.set_data(solution.encode_router_calldata().expect("Failed to encode router calldata"));
                        let transaction = encoder.encode_router_calldata(vec![solution.clone()]).expect("Failed to encode router calldata");
                        let transaction = transaction[0].clone();
                        // let (approval, swap) = batch(network.clone(), solution.clone(), transaction.clone(), header, nonce).unwrap();
                        match batch(network.clone(), solution.clone(), transaction.clone(), header, nonce) {
                            Some((approval, swap)) => {
                                tracing::debug!("--- Raw Transactions ---");
                                tracing::debug!("Approval: {:?}", approval);
                                tracing::debug!("Swap: {:?}", swap);
                                tracing::debug!("--- Formatted Transactions ---");
                                let ep = ExecutionPayload {
                                    swap: SrzTransactionRequest::from(swap),
                                    approve: SrzTransactionRequest::from(approval),
                                };
                                tracing::debug!("Approval: {:?}", ep.approve);
                                tracing::debug!("Swap: {:?}", ep.swap);

                                // encoder.validate_solution

                                return Ok(ep);
                            }
                            None => {
                                tracing::error!("Failed to build transactions");
                            }
                        };
                    }
                    Err(e) => {
                        tracing::error!("Failed to build EVMEncoder: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to get balances of sender: {:?}", e);
        }
    };
    Err("Failed to build transactions".to_string())
}
