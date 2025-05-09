use alloy_chains::NamedChain;
use tycho_simulation::models::Token;

/// Test logs
pub fn tlog()  {
    tracing::info!("Tycho log");
    tracing::debug!("Tycho log");
    tracing::trace!("Tycho log");
    tracing::warn!("Tycho log");
    tracing::error!("Tycho log");
}   

/// Get an environment variable
pub fn get(key: &str) -> String {
    match std::env::var(key) {
        Ok(x) => x,
        Err(_) => {
            panic!("Environment variable not found: {}", key);
        }
    }
}

/// Returns the current timestamp in seconds
pub fn current_timestamp() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("Time went backwards").as_secs()
}

/// Get the Alloy chain based on the network name
pub fn get_alloy_chain(network: String) -> Result<NamedChain, String> {
    match network.as_str() {
        "ethereum" => Ok(NamedChain::Mainnet),
        "base" => Ok(NamedChain::Base),
        "arbitrum" => Ok(NamedChain::Arbitrum),
        _ => {
            tracing::error!("Unsupported network: {}", network);
            Err("Unsupported network".to_string())
        }
    }
}

/// Filter out invalid strings from a vector of strings, that are not ASCII
pub fn filter_valid_strings(input: Vec<Token>) -> Vec<Token> {
    // input.into_iter().filter(|s| !s.symbol.chars().any(|c| c.is_control())).collect()
    input.into_iter()
    .filter(|s| {
        // Ensure the symbol has no control characters and meets any other symbol criteria
        s.symbol.chars().all(|c| c.is_ascii_graphic()) && 
        !s.symbol.chars().any(|c| c.is_control()) &&
        // Check that the address looks valid (e.g., starts with "0x" and is the correct length)
        s.address.to_string().starts_with("0x")
    })
    .collect()
}