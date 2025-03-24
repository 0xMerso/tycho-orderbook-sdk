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

use crate::shd::{
    self,
    data::fmt::SrzProtocolComponent,
    r#static::execution::{APPROVE_FN_SIGNATURE, DEFAULT_APPROVE_GAS, EXEC_DEFAULT_SLIPPAGE},
    types::{ChainSimu, EnvConfig, ExecutionPayload, ExecutionRequest, Network, SrzTransactionRequest},
};

static ROUTER_ADDRESSES: LazyLock<HashMap<ChainSimu, tycho_simulation::tycho_core::Bytes>> = LazyLock::new(|| {
    HashMap::from([
        (
            ChainSimu::Ethereum,
            "0x023eea66B260FA2E109B0764774837629cC41FeF".parse::<tycho_simulation::tycho_core::Bytes>().expect("Failed to create Ethereum router address"),
        ),
        (
            ChainSimu::Base,
            "0x94ebf984511b06bab48545495b754760bfaa566e".parse::<tycho_simulation::tycho_core::Bytes>().expect("Failed to create Base router address"),
        ),
    ])
});

pub fn batch2tx(network: Network, solution: Solution, tx: Transaction, block: alloy::rpc::types::Block, nonce: u64) -> Option<(TransactionRequest, TransactionRequest)> {
    log::info!("Block: {:?}", block);
    let base_fee = block.header.base_fee_per_gas.expect("Base fee not available");
    let max_priority_fee_per_gas = 1_000_000_000u128; // 1 Gwei, not suited for L2s.
    let max_fee_per_gas = base_fee + max_priority_fee_per_gas;
    log::info!("Nonce: {}", nonce);
    // --- Approve Permit2 ---
    let amount: u128 = solution.given_amount.clone().to_string().parse().expect("Couldn't convert given_amount to u128"); // ?
    let args = (Address::from_str(&network.permit2).expect("Couldn't convert to address"), amount);
    let data = tycho_execution::encoding::evm::utils::encode_input(APPROVE_FN_SIGNATURE, args.abi_encode());
    let sender = solution.sender.clone().to_string().parse().expect("Failed to parse sender");
    let approval = TransactionRequest {
        to: Some(alloy::primitives::TxKind::Call(solution.given_token.clone().to_string().parse().expect("Failed to parse given_token"))),
        from: Some(sender),
        value: None,
        input: TransactionInput {
            input: Some(AlloyBytes::from(data)),
            data: None,
        },
        gas: Some(DEFAULT_APPROVE_GAS),
        chain_id: Some(network.chainid),
        max_fee_per_gas: Some(max_fee_per_gas),
        max_priority_fee_per_gas: Some(max_priority_fee_per_gas),
        nonce: Some(nonce),
        ..Default::default()
    };

    let swap = TransactionRequest {
        to: Some(alloy_primitives::TxKind::Call(Address::from_slice(&tx.to))),
        from: Some(sender),
        value: Some(U256::from(0)),
        input: TransactionInput {
            input: Some(AlloyBytes::from(tx.data)),
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

/**
 * Prepare a swap execution.
 */
pub async fn prepare(network: Network, chain: ChainSimu, provider: &RootProvider<Http<Client>>, request: ExecutionRequest, config: EnvConfig, balances: Vec<u128>) -> Option<Solution> {
    log::info!("Preparing swap. Request: {:?}", request);
    let router = ROUTER_ADDRESSES.get(&chain).expect("Router address not found").clone();
    let sum = request.distribution.iter().fold(0., |acc, x| acc + x);
    if !(99. ..=101.).contains(&sum) {
        log::error!("Invalid distribution: {:?}, sum = {}", request.distribution, sum);
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
    let amount_in = (request.amount_in * 10f64.powi(request.input.decimals as i32)) as u128;
    let amount_in = BigUint::from(amount_in);
    let expected_amount_out = (request.expected_amount_out * 10f64.powi(request.output.decimals as i32)) as u128;
    let expected_amount_out = BigUint::from(expected_amount_out);
    let solution: Solution = Solution {
        // Addresses
        sender: tycho_simulation::tycho_core::Bytes::from_str(request.sender.to_lowercase().as_str()).unwrap(),
        receiver: tycho_simulation::tycho_core::Bytes::from_str(request.sender.to_lowercase().as_str()).unwrap(),
        given_token: tycho_simulation::tycho_core::Bytes::from_str(request.input.clone().address.to_lowercase().as_str()).unwrap(),
        checked_token: tycho_simulation::tycho_core::Bytes::from_str(request.output.clone().address.to_lowercase().as_str()).unwrap(),
        // Others fields
        given_amount: amount_in.clone(),
        slippage: Some(EXEC_DEFAULT_SLIPPAGE),
        expected_amount: Some(expected_amount_out),
        exact_out: false,     // It's an exact in solution
        checked_amount: None, // The amount out will not be checked in execution
        swaps: swaps.clone(),
        router_address: router,
        ..Default::default()
    };
    log::info!("Solution: {:?}", solution);
    Some(solution)
}

/**
 * Simulate the approve/swap.
 * ! [WIP] This function is not yet implemented.
 */
pub async fn presimulate(network: Network, nchain: NamedChain, config: EnvConfig, approve: TransactionRequest, swap: TransactionRequest) -> Result<String, String> {
    let wallet = PrivateKeySigner::from_bytes(&B256::from_str(&config.pvkey).expect("Failed to convert swapper pk to B256")).expect("Failed to private key signer");
    let signer = alloy::network::EthereumWallet::from(wallet.clone());
    let prvdww = ProviderBuilder::new().with_chain(nchain).wallet(signer.clone()).on_http(network.rpc.parse().unwrap());
    let payload = SimulatePayload {
        block_state_calls: vec![SimBlock {
            block_overrides: None,
            state_overrides: None,
            calls: vec![approve, swap],
        }],
        trace_transfers: true,
        validation: true,
        return_full_transactions: true,
    };
    // ! [WIP] For some unknown reason, using an async function after initializing EVMEncoderBuilder cause a compiling error
    // So we can't use the following code for now
    // match prvdww.simulate(&payload).await {
    //     Ok(output) => {
    //         for block in output.iter() {
    //             println!("Simulated Block {}:", block.inner.header.number);
    //             for (j, transaction) in block.calls.iter().enumerate() {
    //                 println!("  Transaction {}: Status: {:?}, Gas Used: {}", j + 1, transaction.status, transaction.gas_used);
    //             }
    //         }
    //     }
    //     Err(e) => {
    //         log::error!("Failed to simulate: {:?}", e);
    //     }
    // };

    // Next, see https://docs.propellerheads.xyz/tycho/for-solvers/execution/executing#executing-directly-to-the-executor-contract
    Ok("Simulation successful".to_string())
}

/// Swaps tokens on the specified network.
pub async fn swap(network: Network, request: ExecutionRequest, config: EnvConfig) -> Result<ExecutionPayload, String> {
    let (_, _, chain) = shd::types::chain(network.name.clone()).unwrap();
    let nchain = match network.name.as_str() {
        "ethereum" => NamedChain::Mainnet,
        "base" => NamedChain::Base,
        "arbitrum" => NamedChain::Arbitrum,
        _ => {
            log::error!("Unsupported network: {}", network.name);
            return Err("Unsupported network".to_string());
        }
    };
    let provider = ProviderBuilder::new().with_chain(nchain).on_http(network.rpc.parse().expect("Failed to parse RPC_URL"));
    match super::client::get_erc20_balances(&provider, request.sender.clone(), vec![request.input.address.clone()]).await {
        Ok(balances) => {
            log::info!("Building swap calldata and transactions ...");
            if let Some(solution) = prepare(network.clone(), chain, &provider, request.clone(), config.clone(), balances).await {
                let header: alloy::rpc::types::Block = provider.get_block_by_number(alloy::eips::BlockNumberOrTag::Latest, false).await.unwrap().unwrap();
                let nonce = provider.get_transaction_count(solution.sender.to_string().parse().unwrap()).await.unwrap();
                let encoder = EVMEncoderBuilder::new()
                    .chain(chain)
                    // .initialize_tycho_router_with_permit2(config.pvkey.clone()) // ! Needed ?
                    // .expect("Failed to create encoder builder")
                    .build()
                    .expect("Failed to build encoder");
                let transaction = encoder.encode_router_calldata(vec![solution.clone()]).expect("Failed to encode router calldata");
                let transaction = transaction[0].clone();
                // let (approval, swap) = batch2tx(network.clone(), solution.clone(), transaction.clone(), header, nonce).unwrap();
                match batch2tx(network.clone(), solution.clone(), transaction.clone(), header, nonce) {
                    Some((approval, swap)) => {
                        log::info!("--- Raw Transactions ---");
                        log::info!("Approval: {:?}", approval);
                        log::info!("Swap: {:?}", swap);
                        log::info!("--- Formatted Transactions ---");
                        let ep = ExecutionPayload {
                            swap: SrzTransactionRequest::from(swap),
                            approve: SrzTransactionRequest::from(approval),
                        };
                        log::info!("Approval: {:?}", ep.approve);
                        log::info!("Swap: {:?}", ep.swap);
                        return Ok(ep);
                    }
                    None => {
                        log::error!("Failed to build transactions");
                    }
                };
            }
        }
        Err(e) => {
            log::error!("Failed to get balances of sender: {:?}", e);
        }
    };
    Err("Failed to build transactions".to_string())
}
