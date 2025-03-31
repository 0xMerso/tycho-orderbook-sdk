use crate::types::EnvConfig;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
};

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

/**
 * Read a file and return a Vec<T> where T is a deserializable type
 */
pub fn read<T: DeserializeOwned>(file: &str) -> Vec<T> {
    let mut f = File::open(file).unwrap();
    let mut buffer = String::new();
    f.read_to_string(&mut buffer).unwrap();
    let db: Vec<T> = serde_json::from_str(&buffer).unwrap();
    db
}

/**
 * Write output to file
 */
pub fn save<T: Serialize>(output: Vec<T>, file: &str) {
    let mut file = OpenOptions::new().create(true).write(true).truncate(true).open(file).expect("Failed to open or create file");
    let json = serde_json::to_string(&output).expect("Failed to serialize JSON");
    file.write_all(json.as_bytes()).expect("Failed to write to file");
    file.write_all(b"\n").expect("Failed to write newline to file");
    file.flush().expect("Failed to flush file");
}

/**
 * Write output to file
 */
pub fn save1<T: Serialize>(output: T, file: &str) {
    let mut file = OpenOptions::new().create(true).write(true).truncate(true).open(file).expect("Failed to open or create file");
    let json = serde_json::to_string(&output).expect("Failed to serialize JSON");
    file.write_all(json.as_bytes()).expect("Failed to write to file");
    file.write_all(b"\n").expect("Failed to write newline to file");
    file.flush().expect("Failed to flush file");
}

/// Returns the current timestamp in seconds
pub fn current_timestamp() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("Time went backwards").as_secs()
}
