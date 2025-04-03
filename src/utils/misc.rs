use alloy_chains::NamedChain;

use crate::types::EnvConfig;

/**
 * Default implementation for Env
 */
impl Default for EnvConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl EnvConfig {
    /**
     * Create a new EnvConfig
     */
    pub fn new() -> Self {
        EnvConfig {
            testing: get("TESTING") == "true",
            tycho_api_key: get("TYCHO_API_KEY"),
            network: get("NETWORK"),
            pvkey: get("PV_KEY"),
        }
    }
}

/**
 * Get an environment variable
 */
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
