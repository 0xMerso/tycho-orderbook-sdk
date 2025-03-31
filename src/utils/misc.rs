use crate::types::EnvConfig;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::fs;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
};

pub mod log {

    use fern::colors::{Color, ColoredLevelConfig};
    use log::LevelFilter;

    pub fn logtest() {
        println!("Something 1");
        log::info!("Something 2");
        log::error!("Something 2");
        log::warn!("Something 2");
        log::trace!("Something 2");
        log::debug!("Something 2");
    }

    /**
     * Initialize the logger
     */
    pub fn new(prog: String) {
        let colors = ColoredLevelConfig {
            trace: Color::Cyan,
            debug: Color::Magenta,
            info: Color::Green,
            warn: Color::Red,
            error: Color::BrightRed,
            ..ColoredLevelConfig::new()
        };

        fern::Dispatch::new()
            .format(move |out, message, record| {
                out.finish(format_args!(
                    "{:.23} [{:-<35}:{:<3}] {:<5} {}",
                    chrono::Local::now().to_rfc3339(), // => {:.23} = 2024-08-29T11:53:54.675
                    if record.file().unwrap_or("unknown").len() > 35 {
                        &record.file().unwrap_or("unknown")[record.file().unwrap_or("unknown").len() - 35..]
                    } else {
                        record.file().unwrap_or("unknown")
                    },
                    record.line().unwrap_or(0),
                    colors.color(record.level()),
                    message
                ))
            })
            .chain(std::io::stdout())
            .level(LevelFilter::Off) // Disable all logging from crates
            .level_for(prog.clone(), LevelFilter::Debug) // Launcher logging
            .level_for("tycho_orderbook", LevelFilter::Trace) // Library logging
            .level_for("tests", LevelFilter::Debug) // Library logging
            .apply()
            .unwrap();
    }
}

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
    // log::info!("Saving to file: {}", file);
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
    // log::info!("Saving to file: {}", file);
    let mut file = OpenOptions::new().create(true).write(true).truncate(true).open(file).expect("Failed to open or create file");
    let json = serde_json::to_string(&output).expect("Failed to serialize JSON");
    file.write_all(json.as_bytes()).expect("Failed to write to file");
    file.write_all(b"\n").expect("Failed to write newline to file");
    file.flush().expect("Failed to flush file");
}

/// Load balances from file. For testing purposes
pub fn balances() -> HashMap<String, HashMap<String, u128>> {
    let data = fs::read_to_string("misc/data-back/ethereum.stream-balances.json").expect("Failed to read file");
    let parsed: HashMap<String, HashMap<String, u128>> = serde_json::from_str(&data).expect("JSON parsing failed");
    parsed
}

/// Returns the current timestamp in seconds
pub fn current_timestamp() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("Time went backwards").as_secs()
}
