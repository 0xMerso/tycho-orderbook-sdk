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
            pvkey: get("FAKE_PK"),
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
