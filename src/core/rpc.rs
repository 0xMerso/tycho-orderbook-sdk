use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use alloy::providers::RootProvider;
use alloy::transports::http::Http;
use num_bigint::BigUint;
use reqwest::Client;
use tycho_client::rpc::HttpRPCClient;
use tycho_client::rpc::RPCClient;

use tycho_common::dto::PaginationParams;
use tycho_common::dto::ProtocolStateRequestBody;
use tycho_common::dto::VersionParam;
use tycho_simulation::models::Token;

/// Get the balances of the component in the specified protocol system.
pub async fn get_component_balances(network: Network, cp: String, protosys: String, api_token: Option<String>) -> Option<HashMap<String, u128>> {
    let key: &str = match &api_token {
        Some(t) => t.as_str(),
        None => "sampletoken",
    };
    let client = match HttpRPCClient::new(format!("https://{}", &network.tycho).as_str(), Some(key)) {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("Failed to create client: {:?}", e.to_string());
            return None;
        }
    };
    let (chain, _, _) = types::chain(network.name.clone()).expect("Invalid chain");
    let body = ProtocolStateRequestBody {
        protocol_ids: Some(vec![cp]),
        protocol_system: protosys.to_string(),
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
                    result.insert(c.0.clone().to_string().to_lowercase(), u128::from_str_radix(c.1.to_string().trim_start_matches("0x"), 16).unwrap());
                }
            }
            Some(result)
        }
        Err(e) => {
            tracing::error!("Failed to get protocol states: {:?}", e.to_string());
            None
        }
    }
}

/// Get the tokens from the Tycho API
/// Filters are hardcoded for now.
pub async fn tokens(network: &Network, apikey: String) -> Option<Vec<Token>> {
    match HttpRPCClient::new(format!("https://{}", &network.tycho).as_str(), Some(apikey.as_str())) {
        Ok(client) => {
            let time = std::time::SystemTime::now();
            let (chain, _, _) = types::chain(network.name.clone()).expect("Invalid chain");
            match client.get_all_tokens(chain, Some(100), Some(1), 3000).await {
                Ok(result) => {
                    let mut tokens = vec![];
                    for t in result.iter() {
                        let g = t.gas.first().unwrap_or(&Some(0u64)).unwrap_or_default();
                        if t.symbol.len() >= 20 {
                            continue; // Symbol has been mistaken for a contract address, possibly.
                        }
                        tokens.push(Token {
                            address: tycho_simulation::tycho_core::Bytes::from_str(t.address.clone().to_string().as_str()).unwrap(),
                            decimals: t.decimals as usize,
                            symbol: t.symbol.clone(),
                            gas: BigUint::from(g),
                        });
                    }
                    let elasped = time.elapsed().unwrap().as_millis();
                    tracing::debug!("Took {:?} ms to get {} tokens on {}", elasped, tokens.len(), network.name);
                    Some(tokens)
                }
                Err(e) => {
                    tracing::error!("Failed to get tokens: {:?}", e.to_string());
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

/// Get the balance of the owner for the specified tokens.
pub async fn erc20b(provider: &RootProvider<Http<Client>>, owner: String, tokens: Vec<String>) -> Result<Vec<u128>, String> {
    let mut balances = vec![];
    let client = Arc::new(provider);
    for t in tokens.iter() {
        let contract = IERC20::new(t.parse().unwrap(), client.clone());
        match contract.balanceOf(owner.parse().unwrap()).call().await {
            Ok(res) => {
                let balance = res.balance.to_string().parse::<u128>().unwrap();
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

use crate::types;
use crate::types::AmmType;
use crate::types::Network;
use crate::types::IERC20;
use crate::utils::r#static::maths::BPD;

/// Converts a native fee (as a hex string) into a byte vector representing fee in basis points.
/// The conversion depends on the protocol type:
/// - uniswap_v2_pool: fee is already in basis points (e.g., "0x1e" → 30)
/// - uniswap_v3_pool or uniswap_v4_pool: fee is stored on a 1e6 scale (so 3000 → 30 bps, i.e. divide by 100)
/// - curve: fee is stored on a pow10 scale (e.g., 4000000 becomes 4 bps, so divide by 1_000_000)
/// - balancer_v2_pool: fee is stored on a pow18 scale (e.g., 1*10^15 becomes 10 bps, so divide by 1e14)
pub fn feebps(protocol: String, _id: String, value: String) -> u128 {
    let fee = value.trim_start_matches("0x");
    let fee = u128::from_str_radix(fee, 16).unwrap_or(0);
    let fee = match AmmType::from(protocol.as_str()) {
        AmmType::Pancakeswap | AmmType::Sushiswap | AmmType::UniswapV2 => fee, // Already in bps
        AmmType::UniswapV3 | AmmType::UniswapV4 => fee * (BPD as u128) / 1_000_000,
        AmmType::Curve => 4, // Not implemented, assuming 4 bps by default
        AmmType::Balancer => (fee * (BPD as u128)) / 1e18 as u128,
    };
    fee
}
