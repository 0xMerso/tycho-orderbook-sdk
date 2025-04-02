use std::{collections::HashMap, str::FromStr, sync::LazyLock};

use alloy::{
    primitives::{Address, B256},
    providers::{Provider, ProviderBuilder, ReqwestProvider},
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
use tycho_simulation::protocol::models::ProtocolComponent;

use crate::{
    data::fmt::SrzProtocolComponent,
    types::{self, ChainSimu, EnvConfig, ExecutionPayload, ExecutionRequest, Network, SrzTransactionRequest},
    utils::r#static::{execution, maths::BPD},
};

/// Build 2 transactions for the given solution:
/// 1. Approve the given token to the router address.
/// 2. Swap the given token for the checked token using the router address.
/// The transactions are built using the given network and nonce + 1 on the 2nd transaction.
pub fn prepare(network: Network, solution: Solution, encoded: Transaction, block: alloy::rpc::types::Block, nonce: u64) -> Option<(TransactionRequest, TransactionRequest)> {
    let base_fee = block.header.base_fee_per_gas.expect("Base fee not available");
    let max_priority_fee_per_gas = 1_000_000_000u128; // 1 Gwei, not suited for L2s.
    let max_fee_per_gas = base_fee as u128 + max_priority_fee_per_gas;
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
        gas: Some(300_000u64),
        chain_id: Some(network.chainid),
        max_fee_per_gas: Some(max_fee_per_gas),
        max_priority_fee_per_gas: Some(max_priority_fee_per_gas),
        nonce: Some(nonce + 1),
        ..Default::default()
    };
    Some((approval, swap))
}

/// Build a swap solution Tycho structure
pub async fn solution(network: Network, request: ExecutionRequest, components: Vec<ProtocolComponent>) -> Option<Solution> {
    tracing::debug!("Preparing swap. Request: {:?}", request);
    let router = network.router;
    let sum = request.distribution.iter().fold(0., |acc, x| acc + x);
    if !(99. ..=101.).contains(&sum) {
        tracing::debug!("Invalid distribution: {:?}, sum = {}", request.distribution, sum);
        return None;
    }

    // Failed to encode router calldata: InvalidInput("Split percentage must be less than 1 (100%), got 1")
    // Couting distribution > 0.0
    let single_swap = request.distribution.iter().filter(|&&x| x > 0.0).count() == 1;
    let single_swap_index = request.distribution.iter().position(|&x| x > 0.0).unwrap_or(0);

    tracing::debug!("Single swap: {} | single_swap_index = {}", single_swap, single_swap_index);
    // ! To test
    let distributions: Vec<f64> = request
        .distribution
        .clone()
        .iter()
        .map(|&x| {
            let value = x * BPD;
            let adjusted = match single_swap {
                true => 0.0, // Single swap must have 0% split for the token Bytes(0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2)
                false => {
                    if value > 0.0 {
                        value - 1.0
                    } else {
                        value
                    }
                }
            };
            adjusted / BPD / 100.0
        })
        .collect();

    tracing::debug!("Distribution: {}. Must be < 100. Adjusted distribution = {:?} (0 if single swap)", sum, distributions.clone());
    let mut swaps = vec![];
    for (x, dist) in distributions.iter().enumerate() {
        log::trace!("Distribution #{}: {}", x, dist);
        // let cp = request.components[x].clone(); // get
        let original = components[x].clone(); // get
                                              // let original = SrzProtocolComponent::original(cp.clone(), chain);
        let input = tycho_simulation::tycho_core::Bytes::from_str(request.input.clone().address.to_lowercase().as_str()).unwrap();
        let output = tycho_simulation::tycho_core::Bytes::from_str(request.output.clone().address.to_lowercase().as_str()).unwrap();
        if single_swap && x == single_swap_index {
            swaps.push(tycho_execution::encoding::models::Swap::new(original.clone(), input, output, 0f64));
            break;
        } else if *dist > f64::EPSILON {
            swaps.push(tycho_execution::encoding::models::Swap::new(original.clone(), input, output, *dist));
        }
    }
    let amount_in = BigUint::from((request.amount * 10f64.powi(request.input.decimals as i32)) as u128);
    tracing::debug!("Amount in: {} (pow = {}) of {}", request.amount, amount_in, request.input.symbol.clone());
    let expected = request.expected * 10f64.powi(request.output.decimals as i32);
    let expected_bg = BigUint::from(expected as u128);
    let slippage = execution::EXEC_DEFAULT_SLIPPAGE;
    let checked_amount = expected.clone() * (1.0 - slippage);
    let checked_amount_bg = BigUint::from(checked_amount as u128);
    let solution: Solution = Solution {
        // Addresses
        sender: tycho_simulation::tycho_core::Bytes::from_str(request.sender.to_lowercase().as_str()).unwrap(),
        receiver: tycho_simulation::tycho_core::Bytes::from_str(request.sender.to_lowercase().as_str()).unwrap(),
        given_token: tycho_simulation::tycho_core::Bytes::from_str(request.input.clone().address.to_lowercase().as_str()).unwrap(),
        checked_token: tycho_simulation::tycho_core::Bytes::from_str(request.output.clone().address.to_lowercase().as_str()).unwrap(),
        // Others fields
        given_amount: amount_in.clone(),
        slippage: Some(slippage),
        exact_out: false, // It's an exact in solution
        expected_amount: Some(expected_bg),
        checked_amount: Some(checked_amount_bg), // The amount out will not be checked in execution
        swaps: swaps.clone(),
        // router_address: router, //! wtf ?
        ..Default::default() // native_action => ?
    };
    tracing::debug!("Solution: {:?}", solution);
    Some(solution)
}

/// Broadcast the given transactions to the network
pub async fn broadcast(network: Network, transactions: ExecutionPayload, pk: Option<String>) {
    // Assert private key is provided
    let pk = match pk.clone() {
        Some(pk) => pk,
        None => {
            tracing::error!("Private key not provided");
            return;
        }
    };
    // Build provider and signer
    let alloy_chain = crate::utils::misc::get_alloy_chain(network.name.clone()).expect("Failed to get alloy chain");
    let wallet = PrivateKeySigner::from_bytes(&B256::from_str(&pk).expect("Failed to convert swapper pk to B256")).expect("Failed to private key signer");
    let signer = alloy::network::EthereumWallet::from(wallet.clone());
    let provider = ProviderBuilder::new().with_chain(alloy_chain).wallet(signer.clone()).on_http(network.rpc.parse().unwrap());
    // Approval
    match provider.estimate_gas(&transactions.approve.clone()).await {
        Ok(gas) => {
            tracing::debug!("Approval gas: {:?}", gas);
        }
        Err(e) => {
            tracing::error!("Failed to estimate gas for approval: {:?}", e);
        }
    }

    // --- Simulate ---
    let payload = SimulatePayload {
        block_state_calls: vec![SimBlock {
            block_overrides: None,
            state_overrides: None,
            calls: vec![transactions.approve.clone(), transactions.swap.clone()],
        }],
        trace_transfers: true,
        validation: true,
        return_full_transactions: true,
    };
    match provider.simulate(&payload).await {
        Ok(output) => {
            for block in output.iter() {
                tracing::debug!("Simulated Block {}:", block.inner.header.number);
                for (j, transaction) in block.calls.iter().enumerate() {
                    // tracing::debug!("  Transaction {}: Status: {:?}, Gas Used: {}", j + 1, transaction.status, transaction.gas_used);
                    tracing::debug!("  transaction: {:?}", transaction);
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to simulate: {:?}", e);
        }
    };
}

/// Build swap transactions on the specified network for the given request.
/// Some example: https://github.com/propeller-heads/tycho-execution/blob/main/examples/encoding-example/main.rs
pub async fn build(network: Network, request: ExecutionRequest, components: Vec<ProtocolComponent>, pk: Option<String>) -> Result<ExecutionPayload, String> {
    let (_, _, chain) = types::chain(network.name.clone()).unwrap();
    let tokens = vec![request.input.clone().address, request.output.clone().address];
    let alloy_chain = crate::utils::misc::get_alloy_chain(network.name.clone()).expect("Failed to get alloy chain");
    let provider = ProviderBuilder::new().with_chain(alloy_chain).on_http(network.rpc.parse().expect("Failed to parse RPC_URL"));
    match super::rpc::erc20b(&provider, request.sender.clone(), tokens.clone()).await {
        Ok(balances) => {
            tracing::debug!("Building swap calldata and transactions ...");
            if let Some(solution) = solution(network.clone(), request.clone(), components.clone()).await {
                let header: alloy::rpc::types::Block = provider.get_block_by_number(alloy::eips::BlockNumberOrTag::Latest, false).await.unwrap().unwrap();
                let nonce = provider.get_transaction_count(solution.sender.to_string().parse().unwrap()).await.unwrap();

                match pk {
                    Some(pk) => {
                        let wallet = PrivateKeySigner::from_bytes(&B256::from_str(&pk).expect("Failed to convert swapper pk to B256")).expect("Failed to private key signer");
                        let matching = wallet.address().to_string().eq_ignore_ascii_case(request.sender.clone().as_str());
                        tracing::debug!("Signer imported via pk: {:?} | Request sender: {} | Match = {}", wallet.address(), request.sender.clone(), matching);
                        tracing::debug!("Balances of sender {} => input token: {} and output tokens {}", request.sender, balances[0], balances[1]);
                        std::env::set_var("RPC_URL", network.rpc.clone());
                        let encoder = EVMEncoderBuilder::new()
                            .chain(chain)
                            .initialize_tycho_router_with_permit2(pk.clone())
                            .expect("Failed to create encoder builder");
                        match encoder.build() {
                            Ok(encoder) => {
                                let encoded_tx = encoder.encode_router_calldata(vec![solution.clone()]).expect("Failed to encode router calldata");
                                let encoded_tx = encoded_tx[0].clone();
                                // let (approval, swap) = prepare(network.clone(), solution.clone(), transaction.clone(), header, nonce).unwrap();
                                // Tycho requires this
                                match prepare(network.clone(), solution.clone(), encoded_tx.clone(), header, nonce) {
                                    Some((approval, swap)) => {
                                        // --- Logs ---
                                        tracing::debug!("--- Raw Transactions ---");
                                        tracing::debug!("Approval: {:?}", approval.clone());
                                        tracing::debug!("Swap: {:?}", swap.clone());
                                        tracing::debug!("--- Formatted Transactions ---");
                                        let ep = ExecutionPayload {
                                            approve: approval.clone(),
                                            swap: swap.clone(),
                                            srz_swap: SrzTransactionRequest::from(swap.clone()),
                                            srz_approve: SrzTransactionRequest::from(approval.clone()),
                                        };
                                        tracing::debug!("Approval: {:?}", ep.approve);
                                        tracing::debug!("Swap: {:?}", ep.swap);
                                        tracing::debug!("--- End of Transactions ---");
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
                    None => {
                        tracing::error!("Private key not provided");
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

// Simulate the given transactions
// pub async fn simu(network: Network, nchain: NamedChain, config: EnvConfig, approve: TransactionRequest, swap: TransactionRequest) -> Result<String, String> {
//     let wallet = PrivateKeySigner::from_bytes(&B256::from_str(&config.pvkey).expect("Failed to convert swapper pk to B256")).expect("Failed to private key signer");
//     let signer = alloy::network::EthereumWallet::from(wallet.clone());
//     let prvdww = ProviderBuilder::new().with_chain(nchain).wallet(signer.clone()).on_http(network.rpc.parse().unwrap());
//     let payload = SimulatePayload {
//         block_state_calls: vec![SimBlock {
//             block_overrides: None,
//             state_overrides: None,
//             calls: vec![approve, swap],
//         }],
//         trace_transfers: true,
//         validation: true,
//         return_full_transactions: true,
//     };
//     // For some unknown reason, using an async function after initializing EVMEncoderBuilder cause a compiling error
//     // So we can't use the following code for now
//     match prvdww.simulate(&payload).await {
//         Ok(output) => {
//             for block in output.iter() {
//                 println!("Simulated Block {}:", block.inner.header.number);
//                 for (j, transaction) in block.calls.iter().enumerate() {
//                     println!("  Transaction {}: Status: {:?}, Gas Used: {}", j + 1, transaction.status, transaction.gas_used);
//                 }
//             }
//         }
//         Err(e) => {
//             log::error!("Failed to simulate: {:?}", e);
//         }
//     };
//     Ok("Simulation successful".to_string())
// }
