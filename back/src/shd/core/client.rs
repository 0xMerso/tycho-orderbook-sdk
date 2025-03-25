use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use alloy::providers::RootProvider;
use alloy::transports::http::Http;
use num_bigint::BigUint;
use reqwest::Client;
use tycho_client::rpc::HttpRPCClient;
use tycho_client::rpc::RPCClient;

use tycho_core::dto::ProtocolStateRequestBody;
use tycho_simulation::models::Token;

use crate::shd;
use crate::shd::types::EnvConfig;
use crate::shd::types::Network;
use crate::shd::types::IERC20;

/**
 * Get the balances of the component in the specified protocol system.
 * let ps = "uniswap_v3".to_string();
 * let res = shd::core::client::get_component_balances(network.clone(), config.clone(), "0x391e8501b626c623d39474afca6f9e46c2686649".to_string(), ps).await;
 * dbg!(res);
 */
pub async fn cpbs(network: Network, cp: String, protosys: String) -> Option<HashMap<String, u128>> {
    // log::info!("Getting component balances on {}", network.name);
    let client = match HttpRPCClient::new(format!("https://{}", &network.tycho).as_str(), Some("sampletoken")) {
        Ok(client) => client,
        Err(e) => {
            log::error!("Failed to create client: {:?}", e.to_string());
            return None;
        }
    };
    let (chain, _, _) = shd::types::chain(network.name.clone()).expect("Invalid chain");
    let body = ProtocolStateRequestBody {
        protocol_ids: Some(vec![cp]),
        protocol_system: protosys.to_string(),
        chain,
        include_balances: true,                            // We want to include account balances.
        version: tycho_core::dto::VersionParam::default(), // { timestamp: None, block: None },
        pagination: tycho_core::dto::PaginationParams {
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
            // log::info!("Successfully retrieved {} component balances on {}", component_balances.len(), network.name);
            Some(result)
        }
        Err(e) => {
            log::error!("Failed to get protocol states: {:?}", e.to_string());
            None
        }
    }
}

pub async fn tokens(network: &Network, config: &EnvConfig) -> Option<Vec<Token>> {
    log::info!("Getting all tokens on {}", network.name);
    match HttpRPCClient::new(format!("https://{}", &network.tycho).as_str(), Some(&config.tycho_api_key)) {
        Ok(client) => {
            let time = std::time::SystemTime::now();
            let (chain, _, _) = shd::types::chain(network.name.clone()).expect("Invalid chain");
            match client.get_all_tokens(chain, Some(100), Some(1), 3000).await {
                Ok(result) => {
                    let mut tokens = vec![];
                    for t in result.iter() {
                        let g = t.gas.first().unwrap_or(&Some(0u64)).unwrap_or_default();
                        tokens.push(Token {
                            address: tycho_simulation::tycho_core::Bytes::from_str(t.address.clone().to_string().as_str()).unwrap(),
                            decimals: t.decimals as usize,
                            symbol: t.symbol.clone(),
                            gas: BigUint::from(g),
                        });
                    }
                    let elasped = time.elapsed().unwrap().as_millis();
                    log::info!("Took {:?} ms to get {} tokens on {}", elasped, tokens.len(), network.name);
                    Some(tokens)
                }
                Err(e) => {
                    log::error!("Failed to get tokens: {:?}", e.to_string());
                    None
                }
            }
        }
        Err(e) => {
            log::error!("Failed to create client: {:?}", e.to_string());
            None
        }
    }
}

/**
 * Get the balance of the owner for the specified tokens.
 */
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
                log::error!("Failed to get balance for {}: {:?}", t, e);
                balances.push(0);
            }
        }
    }
    Ok(balances)
}
