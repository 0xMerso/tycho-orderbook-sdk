use crate::{
    data::fmt::SrzToken,
    types::{Network, ProtoSimComp},
};
use alloy::providers::{Provider, ProviderBuilder};
use tycho_simulation::models::Token;

/// Used to retrieve gas price
pub async fn gas_price(provider: String) -> u128 {
    let provider = ProviderBuilder::new().on_http(provider.parse().unwrap());
    provider.get_gas_price().await.unwrap_or_default()
}

/// Find the best path and price between tokens
pub fn pricing(network: Network, ptss: Vec<ProtoSimComp>, atks: Vec<SrzToken>, input: String) -> Option<(f64, Vec<String>)> {
    let mut graph: std::collections::HashMap<String, Vec<(String, f64)>> = std::collections::HashMap::new();
    for state in ptss {
        let addresses: Vec<String> = state.component.tokens.iter().map(|t| t.address.to_lowercase()).collect();
        for token_in in addresses.iter() {
            for token_out in addresses.iter() {
                if token_in != token_out {
                    let base = Token::from(atks.iter().find(|t| t.address.to_lowercase() == token_in.clone()).unwrap().clone());
                    let quote = Token::from(atks.iter().find(|t| t.address.to_lowercase() == token_out.clone()).unwrap().clone());
                    if let Ok(sp) = state.protosim.spot_price(&base, &quote) {
                        graph.entry(token_in.clone()).or_default().push((token_out.clone(), sp));
                    }
                }
            }
        }
    }
    let start = input.to_lowercase();
    let target = network.eth.to_lowercase();
    let unit = 1.0;
    // The stack holds (current token, cumulative rate, path so far)
    let mut stack = vec![(start.clone(), unit, vec![start.clone()])];
    // Remove the global visited set; instead, check for cycles in the current path only.
    while let Some((current, product, path)) = stack.pop() {
        if current == target {
            return Some((product, path));
        }
        if let Some(neighbors) = graph.get(&current) {
            for (next, rate) in neighbors {
                // Only check the current path to avoid cycles.
                if path.contains(&next.to_lowercase()) {
                    continue;
                }
                let mut new_path = path.clone();
                new_path.push(next.to_lowercase());
                stack.push((next.to_lowercase(), product * rate, new_path));
            }
        }
    }
    None
}
