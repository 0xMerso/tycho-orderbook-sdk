use std::collections::{HashMap, HashSet, VecDeque};

use crate::{
    data::fmt::{SrzProtocolComponent, SrzToken},
    types::{ProtoSimComp, ValorisationPath},
};
use tycho_simulation::models::Token;

/// DFS graph traversal method that explores as far as possible along each branch before backtracking
/// Used to price any token to ETH equivalent value, to reflect gas cost
/// But can be used to price any token to any other token
/// Only return the path, not the price
pub fn routing(cps: Vec<SrzProtocolComponent>, input: String, target: String) -> Result<ValorisationPath, String> {
    // (destination token address, component id that provides this conversion)
    let mut graph: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for comp in cps {
        let comp_id = comp.id.clone();
        let addresses: Vec<String> = comp.tokens.iter().map(|t| t.address.to_lowercase()).collect();
        for token_in in &addresses {
            for token_out in &addresses {
                if token_in != token_out {
                    graph.entry(token_in.clone()).or_default().push((token_out.clone(), comp_id.clone()));
                }
            }
        }
    }
    // For debugging: print the graph
    // e.g., log::info!("Graph: {:?}", graph);
    let start = input.to_lowercase();
    let target = target.to_lowercase();
    // Queue items: (current token, token path, component id path)
    let mut queue: VecDeque<(String, Vec<String>, Vec<String>)> = VecDeque::new();
    queue.push_back((start.clone(), vec![start.clone()], vec![]));
    let mut visited: HashSet<String> = HashSet::new();
    while let Some((current, token_path, comp_path)) = queue.pop_front() {
        if current == target {
            return Ok(ValorisationPath { token_path, comp_path });
        }
        if visited.contains(&current) {
            continue;
        }
        visited.insert(current.clone());
        if let Some(neighbors) = graph.get(&current) {
            for (next, comp_id) in neighbors {
                if token_path.contains(next) {
                    continue;
                }
                let mut new_token_path = token_path.clone();
                new_token_path.push(next.clone());
                let mut new_comp_path = comp_path.clone();
                new_comp_path.push(comp_id.clone());
                queue.push_back((next.clone(), new_token_path, new_comp_path));
            }
        }
    }
    Err(format!("No path found from {} to {}", input, target))
}

/// Quote a path of tokens, using components and protosim Tycho functions
/// Used to calculate the price of a path of tokens, mostly to ETH
pub fn quote(pts: Vec<ProtoSimComp>, atks: Vec<SrzToken>, path: Vec<String>) -> Option<f64> {
    // If ETH, return 1. Else, if the path is empty, return None.
    if path.len() == 1 {
        tracing::debug!(" - Path is just ETH. Returning quote of 1.0");
        return Some(1.0);
    } else if path.len() < 2 {
        tracing::error!("🔺 Path is too short: {:?}", path);
        return None;
    }
    let mut cumulative_price = 1.0;
    // For each consecutive pair in the path ...
    for window in path.windows(2) {
        let token_in = window[0].to_lowercase();
        let token_out = window[1].to_lowercase();
        // log::info!("Calculating conversion from {} to {}", token_in, token_out);
        // Find a protocol state that can convert token_in to token_out.
        let mut found = false;
        for state in &pts {
            // Extract the component's token addresses.
            let comp_tokens: Vec<String> = state.component.tokens.iter().map(|t| t.address.to_lowercase()).collect();
            if comp_tokens.contains(&token_in) && comp_tokens.contains(&token_out) {
                // Resolve the tokens from the global list.
                let base = Token::from(atks.iter().find(|t| t.address.to_lowercase() == token_in).unwrap().clone());
                let quote = Token::from(atks.iter().find(|t| t.address.to_lowercase() == token_out).unwrap().clone());
                match state.protosim.spot_price(&base, &quote) {
                    Ok(rate) => {
                        // log::info!("Found rate {} for {} -> {}", rate, token_in, token_out);
                        cumulative_price *= rate;
                        found = true;
                        break;
                    }
                    Err(_e) => {
                        // log::info!("State cannot convert {} -> {}: {}", token_in, token_out, e);
                    }
                }
            }
        }
        if !found {
            tracing::warn!("🔺 Quote error: no conversion path found for {} -> {}", token_in, token_out);
            return None;
        }
    }
    tracing::debug!(" - One unit of token ({:?} to {:?}) quoted to ETH = {}", path.first(), path.last(), cumulative_price);
    Some(cumulative_price)
}
