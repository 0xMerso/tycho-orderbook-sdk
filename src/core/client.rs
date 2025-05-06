use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use alloy::providers::Provider;
use alloy::providers::ProviderBuilder;
use alloy::providers::RootProvider;
use alloy::transports::http::Http;
use alloy_primitives::Address;
use num_bigint::BigUint;
use reqwest::Client;
use tycho_client::rpc::HttpRPCClient;
use tycho_client::rpc::RPCClient;

use tycho_common::dto::PaginationParams;
use tycho_common::dto::ProtocolStateRequestBody;
use tycho_common::dto::VersionParam;
use tycho_simulation::models::Token;

use crate::types;
use crate::types::CoinGeckoResponse;
use crate::types::IChainLinkPF;
use crate::types::Network;
use crate::types::IERC20;
use crate::utils::misc::filter_valid_strings;
use crate::utils::r#static::endpoints::COINGECKO_ETH_USD;

/// ========================================================================================= Tycho Client =============================================================================================
/// Get the balances of the component in the specified protocol system.
/// Returns a HashMap of component addresses and their balances.
/// Balance is returned as a u128, with decimals.
pub async fn get_component_balances(client: &HttpRPCClient, network: Network, cp: String, protosys: String) -> Option<HashMap<String, u128>> {
    let (chain, _, _) = types::chain(network.name.clone()).expect("Invalid chain");
    let body = ProtocolStateRequestBody {
        protocol_ids: Some(vec![cp.clone()]),
        protocol_system: protosys.to_string(), // Single, so cannot use protocol_ids vec of different protocols ?
        chain,
        include_balances: true,           // We want to include account balances.
        version: VersionParam::default(), // { timestamp: None, block: None },
        pagination: PaginationParams {
            page: 0,        // Start at the first page.
            page_size: 100, // Maximum page size supported is 100.
        },
    };
    match client.get_protocol_states(&body).await {
        Ok(response) => {
            let component_balances = response.states.into_iter().map(|state| state.balances.clone()).collect::<Vec<_>>();
            let mut result = HashMap::new();
            for cb in component_balances.iter() {
                for c in cb.iter() {
                    let b = u128::from_str_radix(c.1.to_string().trim_start_matches("0x"), 16);
                    if let Ok(b) = b {
                        result.insert(c.0.clone().to_string().to_lowercase(), b);
                    }
                }
            }
            Some(result)
        }
        Err(e) => {
            tracing::error!("Failed to get protocol states: {}: {:?}", cp.clone(), e.to_string());
            None
        }
    }
}

/// Get the tokens from the Tycho API
/// Filters are hardcoded for now.
pub async fn tokens(network: &Network, apikey: String) -> Option<Vec<Token>> {
    tracing::info!("Getting tokens for network {}", network.name);
    match HttpRPCClient::new(format!("https://{}", &network.tycho).as_str(), Some(apikey.as_str())) {
        Ok(client) => {
            let time = std::time::SystemTime::now();
            let (chain, _, _) = types::chain(network.name.clone()).expect("Invalid chain");
            match client.get_all_tokens(chain, Some(100), Some(1), 500).await {
                Ok(result) => {
                    let mut tokens = vec![];
                    for t in result.iter() {
                        let g = t.gas.first().unwrap_or(&Some(0u64)).unwrap_or_default();
                        if t.symbol.len() >= 20 {
                            continue; // Symbol has been mistaken for a contract address, possibly.
                        }
                        if let Ok(addr) = tycho_simulation::tycho_core::Bytes::from_str(t.address.clone().to_string().as_str()) {
                            tokens.push(Token {
                                address: addr,
                                decimals: t.decimals as usize,
                                symbol: t.symbol.clone(),
                                gas: BigUint::from(g),
                            });
                        }
                    }
                    tokens = filter_valid_strings(tokens);
                    let elasped = time.elapsed().unwrap_or_default().as_millis();
                    tracing::debug!("Took {:?} ms to get {} tokens on {}", elasped, tokens.len(), network.name);

                    Some(tokens)
                }
                Err(e) => {
                    tracing::error!("Failed to get tokens on network {}: {:?}", network.name, e.to_string());
                    None
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to create client: {:?}", e.to_string());
            None
        }
    }
}

/// Get the tokens from the Tycho API
/// Filters are hardcoded for now.
pub fn build_tycho_client(network: &Network, key: Option<String>) -> Result<HttpRPCClient, anyhow::Error> {
    let key: &str = match &key {
        Some(t) => t.as_str(),
        None => "sampletoken",
    };
    match HttpRPCClient::new(format!("https://{}", &network.tycho).as_str(), Some(key)) {
        Ok(client) => Ok(client),
        Err(e) => {
            tracing::error!("Failed to create client: {:?}", e.to_string());
            Err(anyhow::anyhow!("Failed to create client: {:?}", e.to_string()))
        }
    }
}

/// =========================================================================================== HTTP Provider/RPC ======================================================================================
/// Retrieve eth usd price
pub async fn coingecko() -> Option<f64> {
    match reqwest::get(COINGECKO_ETH_USD).await {
        Ok(response) => match response.json::<CoinGeckoResponse>().await {
            Ok(data) => Some(data.ethereum.usd),
            Err(_) => None,
        },
        Err(_) => None,
    }
}

/// Used to retrieve the block number
pub async fn get_latest_block(provider: String) -> u64 {
    let provider = ProviderBuilder::new().on_http(provider.parse().unwrap());
    provider.get_block_number().await.unwrap_or_default()
}

/// Get the balance of the owner for the specified tokens.
pub async fn erc20b(provider: &RootProvider<Http<Client>>, owner: String, tokens: Vec<String>) -> Result<Vec<u128>, String> {
    let mut balances = vec![];
    let client = Arc::new(provider);
    for t in tokens.iter() {
        let contract = IERC20::new(t.parse().unwrap(), client.clone());
        match contract.balanceOf(owner.parse().unwrap()).call().await {
            Ok(res) => {
                let balance = res.balance.to_string().parse::<u128>().unwrap_or_default();
                balances.push(balance);
            }
            Err(e) => {
                tracing::error!("Failed to get balance for {}: {:?}", t, e);
                balances.push(0);
            }
        }
    }
    Ok(balances)
}

/// Fetch the price of and oracle, in this case of the 'gas_token' of a network
/// Assume the oracle in under the 'Chainlink' interface
/// Unwrap are assumed safe, given the configuration SDK is correct.
pub async fn get_eth_usd_chainlink(rpc: String, feed: String) -> Option<f64> {
    tracing::debug!("Fetching price from chainlink oracle: {} with RPC = {}", feed.clone(), rpc.clone());
    let pfeed: Address = feed.clone().parse().unwrap();
    let provider = ProviderBuilder::new().on_http(rpc.parse().unwrap());
    let client = Arc::new(provider);
    let oracle = IChainLinkPF::new(pfeed, client.clone());
    let price = oracle.latestAnswer().call().await;
    let precision = oracle.decimals().call().await;
    match (price, precision) {
        (Ok(price), Ok(precision)) => {
            let power = 10f64.powi(precision._0 as i32);
            let price = price._0.as_u64() as f64 / power;
            tracing::debug!("Price from chainlink oracle: {}", price);
            Some(price)
        }
        _ => {
            tracing::error!("Error fetching price from chainlink oracle");
            None
        }
    }
}
