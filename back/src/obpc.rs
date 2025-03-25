// client.rs

use std::{collections::HashMap, sync::Arc};
use tap2::shd::obp::{OBPConfig, OBPEvent, OBP};
use tap2::shd::types::{SharedTychoStreamState, TychoStreamState};
use tokio::sync::RwLock;


use futures::StreamExt;
use tap2::shd;
use tap2::shd::types::EnvConfig;
use tap2::shd::types::Network;



#[tokio::main]
async fn main() {
    shd::utils::misc::log::new("obpc".to_string());
    dotenv::from_filename(".env.prod").ok(); // Use .env.ex for testing
    let config = EnvConfig::new();
    log::info!("Launching OBP Client on {} | 🧪 Testing mode: {:?}", config.network, config.testing);
    let path = "src/shd/config/networks.json".to_string();
    let networks: Vec<Network> = shd::utils::misc::read(&path);
    let network = networks.clone().into_iter().filter(|x| x.enabled).find(|x| x.name == config.network).expect("Network not found or not enabled");
    log::info!("Tycho Stream for '{}' network", network.name.clone());
    // Create shared state for the protocol stream
    let shared_state: SharedTychoStreamState = Arc::new(RwLock::new(TychoStreamState {
        protosims: HashMap::new(),  // Customize with your actual types
        components: HashMap::new(), // Customize with your actual types
        initialised: false,
    }));
    // Create the OBP provider from the protocol stream builder and shared state.
    let psb = shd::obp::prebuild(network.clone(), config.clone()).await;
    let obp = OBP::new(psb, OBPConfig::default(), shared_state).await.expect("Failed to build OBP");
    let mut stream = obp.stream;
    // Loop indefinitely over the stream, printing received events.
    while let Some(event) = stream.recv().await {
        match event {
            // OBPEvent::BlockUpdate(update) => {
            //     println!("Received block update: {:?}", update);
            // }
            OBPEvent::BlockNumber(number) => {
                log::info!("OBPEvent: Received block number: {:?}", number);
            }
            OBPEvent::Error(err) => {
                log::error!("OBPEvent: Error: {:?}", err);
            }
        }
    }
}
