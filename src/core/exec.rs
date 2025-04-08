use std::{collections::HashMap, str::FromStr};

use alloy::{
    primitives::{Address, B256},
    providers::{Provider, ProviderBuilder},
    rpc::types::{
        simulate::{SimBlock, SimulatePayload},
        TransactionInput, TransactionRequest,
    },
    signers::local::PrivateKeySigner,
    sol_types::SolValue,
};
use num_bigint::BigUint;
use tycho_execution::encoding::{
    evm::encoder_builder::EVMEncoderBuilder,
    models::{Solution, Transaction},
    tycho_encoder::TychoEncoder,
};

use alloy_primitives::{Bytes as AlloyBytes, U256};
use tycho_simulation::protocol::models::ProtocolComponent;

use crate::{
    data::fmt::SrzProtocolComponent,
    types::{self, ExecutedPayload, ExecutionRequest, Network, PayloadToExecute},
    utils::r#static::{execution, maths::BPD},
};

/// Get the original components from the list of components
/// Used when Tycho packages require the exact components
/// Conversion from:: SrzProtocolComponent to ProtocolComponent doesn't work. Idk why.
pub fn get_original_components(originals: HashMap<String, ProtocolComponent>, targets: Vec<SrzProtocolComponent>) -> Vec<ProtocolComponent> {
    let mut filtered = Vec::with_capacity(targets.len());
    for cp in targets.clone().iter().enumerate() {
        let tgt = cp.1.id.to_string().to_lowercase();
        if let Some(original) = originals.get(&tgt) {
            filtered.push(original.clone());
        } else {
            tracing::warn!("OBP Event: Error: Component {} not found in the original list, anormal !", tgt);
        }
    }
    if filtered.len() != targets.len() {
        tracing::error!("Execution error: not all components found in the original list, anormal !");
    }
    let order: HashMap<String, usize> = targets.iter().enumerate().map(|(i, item)| (item.id.to_string().to_lowercase(), i)).collect();
    filtered.sort_by_key(|item| order.get(&item.id.to_string().to_lowercase()).copied().unwrap_or(usize::MAX));
    // --- Tmp Debug ---
    // for o in filtered.iter() {
    //     tracing::trace!(" - originals : {}", o.id);
    //     let attributes = o.static_attributes.clone();
    //     for a in attributes.iter() {
    //         tracing::trace!("   - {}: {}", a.0, a.1);
    //     }
    // }
    // for t in targets.iter() {
    //     tracing::trace!(" - targets   : {}", t.id);
    //     let attributes = t.static_attributes.clone();
    //     for a in attributes.iter() {
    //         tracing::trace!("   - {}: {}", a.0, a.1);
    //     }
    // }
    filtered
}

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
pub async fn solution(_network: Network, request: ExecutionRequest, components: Vec<ProtocolComponent>) -> Option<Solution> {
    tracing::debug!("Preparing swap. Sender: {} | Orderbook: {:?}", request.sender, request.tag);
    let sum = request.distribution.iter().fold(0., |acc, x| acc + x);
    if !(99. ..=101.).contains(&sum) {
        tracing::debug!("Invalid distribution: {:?}, sum = {}", request.distribution, sum);
        return None;
    }
    // Multiple checks are performed by the Tycho encoder, including
    // - Failed to encode router calldata: InvalidInput("Split percentage must be less than 1 (100%), got 1")
    let single_swap = request.distribution.iter().filter(|&&x| x > 0.0).count() == 1; // Couting distribution > 0.0
    let single_swap_index = request.distribution.iter().position(|&x| x > 0.0).unwrap_or(0);
    tracing::debug!("Single swap: {} | single_swap_index = {}", single_swap, single_swap_index);

    // Multi (= splitted, not multi hop) trade
    let distributions: Vec<f64> = request
        .distribution
        .clone()
        .iter()
        .map(|&x| {
            let value = x * BPD;
            let adjusted = match single_swap {
                true => 0.0, // Single swap must have 0% split, the 100% remaining is considered as the swap amount
                false => {
                    if value > 0.0 {
                        value - 1.0 // Just to make sure it never exceeds 100%
                    } else {
                        value
                    }
                }
            };
            adjusted / BPD / 100.0
        })
        .collect();

    tracing::debug!(
        "Initial distribution sum: {} (should be close to 100). Adjusted distribution = {:?} (full 0 if single swap)",
        sum,
        distributions.clone()
    );
    // Prepare the swaps, adding a swap for each distribution > 0
    // Exact ProtocolComponent structure is needed for the Tycho encoder, it doesn't work to partially convert a SrzProtocolComponent to ProtocolComponent
    let mut swaps = vec![];
    for (x, dist) in distributions.iter().enumerate() {
        // log::trace!("Distribution #{}: {}", x, dist);
        let original = components[x].clone(); // get
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
    tracing::debug!("Req.Amount: {} (pow = {}) of {}", request.amount, amount_in, request.input.symbol.clone());
    let expected = request.expected * 10f64.powi(request.output.decimals as i32);
    let expected_bg = BigUint::from(expected as u128);
    let slippage = execution::EXEC_DEFAULT_SLIPPAGE;
    let checked_amount = expected * (1.0 - slippage);
    let checked_amount_bg = BigUint::from(checked_amount as u128);
    tracing::debug!("Expected: {} of {} | Checked: {}", expected, request.output.symbol.clone(), checked_amount);
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
        ..Default::default()
    };
    // tracing::trace!("Solution: {:?}", solution);
    Some(solution)
}

/// Broadcast the given transactions to the network
pub async fn broadcast(network: Network, transactions: PayloadToExecute, pk: Option<String>) -> ExecutedPayload {
    let mut br = ExecutedPayload::default();
    // --- Assert private key is provided ---
    let pk = match pk.clone() {
        Some(pk) => pk,
        None => {
            tracing::error!("Private key not provided");
            return br;
        }
    };
    // --- Build provider and signer ---
    let alloy_chain = crate::utils::misc::get_alloy_chain(network.name.clone()).expect("Failed to get alloy chain");
    let wallet = PrivateKeySigner::from_bytes(&B256::from_str(&pk).expect("Failed to convert swapper pk to B256")).expect("Failed to private key signer");
    let signer = alloy::network::EthereumWallet::from(wallet.clone());
    let provider = ProviderBuilder::new().with_chain(alloy_chain).wallet(signer.clone()).on_http(network.rpc.parse().unwrap());
    let sender = transactions.swap.from.unwrap_or_default().to_string().to_lowercase();
    let matching = wallet.address().to_string().eq_ignore_ascii_case(sender.clone().as_str());
    tracing::trace!(
        "Signer imported via pk: {:?} | Request sender: {:?} | Match = {}",
        wallet.address(),
        transactions.swap.from.clone(),
        matching
    );

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
            let mut green = true;
            for block in output.iter() {
                tracing::trace!("Simulated Block {}:", block.inner.header.number);
                for (x, tx) in block.calls.iter().enumerate() {
                    tracing::trace!("  Tx #{}: Gas: {} | Simulation status: {}", x, tx.gas_used, tx.status);
                    if !tx.status {
                        tracing::error!("Simulation failed for tx #{}. No broadcast.", x);
                        green = false;
                    }
                }
            }
            if green {
                tracing::debug!("Broadcasting to RPC URL: {}", network.rpc);
                //  --- Broadcast Approval ---
                match provider.send_transaction(transactions.approve).await {
                    Ok(approve) => {
                        br.approve.sent = true;
                        tracing::debug!("Waiting for receipt on approval tx: {:?}", approve.tx_hash());
                        br.approve.hash = approve.tx_hash().to_string();
                        tracing::debug!("Explorer: {}tx/{}", network.exp, approve.tx_hash());
                        match approve.get_receipt().await {
                            Ok(receipt) => {
                                tracing::debug!("Approval receipt: status: {:?}", receipt.status());
                                br.approve.status = receipt.status();
                                if receipt.status() {
                                    tracing::debug!("Approval transaction succeeded");
                                    // --- Broadcast Swap ---
                                    br.swap.sent = true;
                                    match provider.send_transaction(transactions.swap).await {
                                        Ok(swap) => {
                                            br.swap.hash = swap.tx_hash().to_string();
                                            tracing::debug!("Waiting for receipt on swap tx: {:?}", swap.tx_hash());
                                            tracing::debug!("Explorer: {}tx/{}", network.exp, swap.tx_hash());
                                            match swap.get_receipt().await {
                                                Ok(receipt) => {
                                                    tracing::debug!("Swap receipt: status: {:?}", receipt.status());
                                                    br.swap.status = receipt.status();
                                                    if receipt.status() {
                                                        tracing::debug!("Swap transaction succeeded");
                                                    } else {
                                                        tracing::error!("Swap transaction failed");
                                                    }
                                                }
                                                Err(e) => {
                                                    tracing::error!("Failed to wait for swap transaction: {:?}", e);
                                                    br.swap.error = Some(e.to_string());
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to send swap transaction: {:?}", e);
                                            br.swap.error = Some(e.to_string());
                                        }
                                    }
                                } else {
                                    tracing::error!("Approval transaction failed");
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to wait for approval transaction: {:?}", e);
                                br.approve.error = Some(e.to_string());
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to send approval transaction: {:?}", e);
                        br.approve.error = Some(e.to_string());
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to simulate: {:?}", e);
        }
    };
    br
}

/// Build swap transactions on the specified network for the given request.
/// Some example: https://github.com/propeller-heads/tycho-execution/blob/main/examples/encoding-example/main.rs
pub async fn build(network: Network, request: ExecutionRequest, native: Vec<ProtocolComponent>, pk: Option<String>) -> Result<PayloadToExecute, String> {
    tracing::debug!("Building transactions for request: {:?} | Private key provided: {}", request, pk.is_some());
    let (_, _, chain) = types::chain(network.name.clone()).unwrap();
    let tokens = vec![request.input.clone().address, request.output.clone().address];
    let achain = crate::utils::misc::get_alloy_chain(network.name.clone()).expect("Failed to get alloy chain");
    let provider = ProviderBuilder::new().with_chain(achain).on_http(network.rpc.parse().expect("Failed to parse RPC_URL"));

    // --- Check if the sender has enough balance of input token ---
    match super::client::erc20b(&provider, request.sender.clone(), tokens.clone()).await {
        Ok(balances) => {
            tracing::debug!("Balances of sender {}: Input: {} | Output: {}", request.sender, balances[0], balances[1]);
            let amount = (request.amount * 10f64.powi(request.input.decimals as i32)) as u128;
            if amount > balances[0] {
                tracing::error!("Not enough balance for input token: need {} but sender has {}", amount, balances[0]);
                return Err("Not enough balance for input token".to_string());
            }
        }
        Err(e) => {
            tracing::error!("Failed to get balances of sender: {:?}", e);
        }
    };

    tracing::debug!("Building swap calldata and transactions ...");
    if let Some(solution) = solution(network.clone(), request.clone(), native.clone()).await {
        let header: alloy::rpc::types::Block = provider.get_block_by_number(alloy::eips::BlockNumberOrTag::Latest, false).await.unwrap().unwrap();
        let nonce = provider.get_transaction_count(solution.sender.to_string().parse().unwrap()).await.unwrap();
        std::env::set_var("RPC_URL", network.rpc.clone());
        // Need a strategy, else we get: FatalError("Please set the chain and strategy before building the encoder")
        let encoder = match pk {
            Some(pk) => EVMEncoderBuilder::new().chain(chain).initialize_tycho_router_with_permit2(pk.clone()),
            None => EVMEncoderBuilder::new().chain(chain).initialize_tycho_router(),
        };
        match encoder {
            Ok(encoder) => {
                match encoder.build() {
                    Ok(encoder) => {
                        match encoder.encode_router_calldata(vec![solution.clone()]) {
                            Ok(encoded_tx) => {
                                let encoded_tx = encoded_tx[0].clone();
                                match prepare(network.clone(), solution.clone(), encoded_tx.clone(), header, nonce) {
                                    Some((approval, swap)) => {
                                        let ep = PayloadToExecute {
                                            approve: approval.clone(),
                                            swap: swap.clone(),
                                        };
                                        // --- Logs ---
                                        // tracing::debug!("--- Raw Transactions ---");
                                        // tracing::debug!("Approval: {:?}", approval.clone());
                                        // tracing::debug!("Swap: {:?}", swap.clone());
                                        // tracing::debug!("--- Formatted Transactions ---");
                                        // tracing::debug!("Approval: {:?}", ep.approve);
                                        // tracing::debug!("Swap: {:?}", ep.swap);
                                        // tracing::debug!("--- End of Transactions ---");
                                        return Ok(ep);
                                    }
                                    None => {
                                        tracing::error!("Failed to prepare transactions");
                                    }
                                };
                            }
                            Err(e) => {
                                tracing::error!("Failed to encode router calldata: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to build EVMEncoder: {:?}", e);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to build EVMEncoder: {:?}", e);
            }
        };
    }

    Err("Failed to build transactions".to_string())
}
