#![allow(unused)] // silence unused warnings while exploring (to comment out)

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{error::Error, time::Duration};
use tokio::time::sleep;

use redis::{
    aio::MultiplexedConnection,
    from_redis_value,
    streams::{StreamRangeReply, StreamReadOptions, StreamReadReply},
    AsyncCommands, Client, RedisError,
};

use crate::types::SyncState;

/// ================ Naming Convention ================
/// ALL             : <network>                            : One struct for eveything related to a network
/// NetworkConfig   : <network>.config                     : The network configuration
/// ALL ActiveUser  : <network>:<protocol>:cdps            : ActiveUser per network AND per protocol
/// OracLe Snap     : <network>:<protocol>:prices          : One Oracle snapshot at a block, for each reserve
/// One ActiveUser  : <network>:<protocol>:cdps:<useraddr> : One ActiveUser

pub async fn ping() {
    let co = connect().await;
    match co {
        Ok(mut co) => {
            let pong: redis::RedisResult<String> = redis::cmd("PING").query_async(&mut co).await;
            match pong {
                Ok(pong) => {
                    log::info!("📕 Redis Ping Good");
                }
                Err(e) => {
                    panic!("Redis PING Error: {}", e);
                }
            }
        }
        Err(e) => {
            panic!("Redis PING Error: {}", e);
        }
    }
}

/**
 * Connect to Redis
 */
pub async fn connect() -> Result<MultiplexedConnection, RedisError> {
    let endpoint = std::env::var("REDIS_HOST");
    let endpoint = match endpoint {
        Ok(endpoint) => endpoint,
        Err(_) => "127.0.0.1:7777".to_string(),
    };
    let endpoint = format!("redis://{}", endpoint);
    // log::info!("Redis endpoint: {}", endpoint);
    let client = Client::open(endpoint);
    match client {
        Ok(client) => client.get_multiplexed_tokio_connection().await,
        Err(e) => {
            log::error!("Redis Client Error: {}", e);
            Err(e)
        }
    }
}

/**
 * Get the status of the Redis db for a given network
 */
pub async fn status(key: String) -> SyncState {
    let status = get::<u128>(key.as_str()).await;
    match status {
        Some(status) => match status {
            1 => SyncState::Down,
            2 => SyncState::Launching,
            3 => SyncState::Syncing,
            4 => SyncState::Running,
            _ => SyncState::Error,
        },
        None => SyncState::Error,
    }
}

/**
 * Infinite waiting for the status 'Running' for a given network
 */
pub async fn wstatus(key: String, object: String) {
    let time = std::time::SystemTime::now();
    log::info!("Waiting Redis Synchro");
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
        let status = status(key.clone()).await;
        log::info!("Waiting for '{object}'. Current status: {:?}", status);
        if let SyncState::Running = status {
            let elasped = time.elapsed().unwrap().as_millis();
            log::info!("wstatus: redis db is ready. Took {} ms to sync", elasped);
            break;
        }
    }
}

/**
 * Delete a JSON object from Redis
 */
pub async fn delete(key: &str) {
    let co = connect().await;
    match co {
        Ok(mut co) => {
            let deletion: redis::RedisResult<()> = redis::cmd("DEL").arg(key).query_async(&mut co).await;
            if let Err(err) = deletion {
                log::error!("Failed to delete JSON object with key '{}': {}", key, err);
            }
        }
        Err(e) => {
            log::error!("Redis connection error: {}", e);
        }
    }
}

/**
 * Save a JSON object to Redis
 */
pub async fn set<T: Serialize>(key: &str, data: T) {
    let data = serde_json::to_string(&data);
    match data {
        Ok(data) => {
            let co = connect().await;
            // let client = Client::open("redis://redis/");
            match co {
                Ok(mut co) => {
                    let result: redis::RedisResult<()> = redis::cmd("SET").arg(key).arg(data.clone()).query_async(&mut co).await;
                    if let Err(err) = result {
                        log::error!("📕 Failed to set value for key '{}': {}", key, err);
                    }
                    // log::info!("📕 Set succeeded for key '{}'", key);
                }

                Err(e) => {
                    log::error!("📕 Redis connection error: {}", e);
                }
            }
        }
        Err(err) => {
            log::error!("📕 Failed to serialize JSON object: {}", err);
        }
    }
}

/**
 * Get a JSON object from Redis
 */
pub async fn get<T: Serialize + DeserializeOwned>(key: &str) -> Option<T> {
    let time = std::time::SystemTime::now();
    let co = connect().await;
    match co {
        Ok(mut co) => {
            let result: redis::RedisResult<String> = redis::cmd("GET").arg(key).query_async(&mut co).await;
            match result {
                Ok(value) => {
                    let elasped = time.elapsed().unwrap().as_millis();
                    match serde_json::from_str(&value) {
                        Ok(value) => {
                            // log::info!("📕 Get succeeded for key '{}'. Elapsed: {}ms", key, elasped);
                            Some(value)
                        }
                        Err(err) => {
                            log::error!("📕 Failed to deserialize JSON object: {}", err);
                            None
                        }
                    }
                }
                Err(err) => {
                    // log::error!("📕 Failed to get value for key '{}': {}", key, err);
                    None
                }
            }
        }
        Err(e) => {
            log::error!("📕 Redis connection error: {}", e);
            None
        }
    }
}
