use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
};

use serde::{de::DeserializeOwned, Serialize};

use crate::shd::types::EnvConfig;

pub mod log {

    use fern::colors::{Color, ColoredLevelConfig};
    use log::LevelFilter;

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
                    "{:.23} [{:-<35}:{:<3}] {} {}",
                    chrono::Local::now().to_rfc3339(), // => {:.23} = 2024-08-29T11:53:54.675
                    if record.file().unwrap_or("unknown").len() > 35 {
                        &record.file().unwrap_or("unknown")[record.file().unwrap_or("unknown").len() - 35..]
                    } else {
                        record.file().unwrap_or("unknown")
                    },
                    // record.file().unwrap_or("unknown"),
                    record.line().unwrap_or(0),
                    colors.color(record.level()),
                    message
                ))
            })
            .chain(std::io::stdout())
            .level(LevelFilter::Off) // Disable all logging from crates
            .level_for(prog.clone(), LevelFilter::Info) // Launcher logging
            .level_for("shd", LevelFilter::Info) // Library logging
            .level_for("api", LevelFilter::Info) // API logging
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
            tycho_url: get("TYCHO_URL"),
            tycho_api_key: get("TYCHO_API_KEY"),
            network: get("NETWORK"),
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
