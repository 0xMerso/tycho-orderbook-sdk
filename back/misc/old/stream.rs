use std::collections::HashMap;
use std::sync::Arc;

use futures::StreamExt;
use tap2::shd;
use tap2::shd::data::fmt::SrzEVMPoolState;
use tap2::shd::data::fmt::SrzProtocolComponent;
use tap2::shd::data::fmt::SrzToken;
use tap2::shd::data::fmt::SrzUniswapV2State;
use tap2::shd::data::fmt::SrzUniswapV3State;
use tap2::shd::data::fmt::SrzUniswapV4State;
use tap2::shd::r#static::data::keys;
use tap2::shd::r#static::filter::NULL_ADDRESS;
use tap2::shd::types::AmmType;
use tap2::shd::types::EnvConfig;
use tap2::shd::types::Network;
use tap2::shd::types::OrderbookBuilder;
use tap2::shd::types::SharedTychoStreamState;
use tap2::shd::types::SyncState;
use tap2::shd::types::TychoStreamState;
use tokio::sync::RwLock;
use tycho_simulation::evm::protocol::uniswap_v3::state::UniswapV3State;
use tycho_simulation::evm::protocol::uniswap_v4::state::UniswapV4State;

use tycho_simulation::evm::{
    engine_db::tycho_db::PreCachedDB,
    protocol::{uniswap_v2::state::UniswapV2State, vm::state::EVMPoolState},
};

pub mod api;

/**
 * Stream the entire state from each AMMs, with TychoStreamBuilder.
 */
async fn stream(network: Network, shdstate: SharedTychoStreamState, config: EnvConfig) {
    log::info!(" 1️⃣  Launching ProtocolStreamBuilder task for {}", network.name);
    let (_, _, chain) = shd::types::chain(network.name.clone()).expect("Invalid chain");
    'retry: loop {
        let tokens = shd::core::rpc::tokens(&network, &config).await.unwrap();
        let mut hmt = HashMap::new();
        tokens.iter().for_each(|t| {
            hmt.insert(t.address.clone(), t.clone());
        });
        let srztokens = tokens.iter().map(|t| SrzToken::from(t.clone())).collect::<Vec<SrzToken>>();
        // for t in srztokens.iter() {
        //     if t.symbol.to_lowercase().contains("btc") {
        //         log::info!("Token: {} at {}", t.symbol, t.address.to_lowercase());
        //     }
        // }
        let key = keys::stream::tokens(network.name.clone());
        shd::data::redis::set(key.as_str(), srztokens.clone()).await;
        log::info!("Connecting to >>> ProtocolStreamBuilder <<< at {} on {:?} ...\n", network.tycho, chain);
        let builder = OrderbookBuilder::new(network.clone(), config.clone(), Some(tokens)).await;
        match builder.psb.build().await {
            Ok(mut stream) => {
                while let Some(msg) = stream.next().await {
                    match msg {
                        Ok(msg) => {
                            log::info!(
                                "🔸 Stream: block # {} with {} state, {} new and {} removed",
                                msg.block_number,
                                msg.states.len(),
                                msg.new_pairs.len(),
                                msg.removed_pairs.len()
                            );
                            shd::data::redis::set(keys::stream::latest(network.name.clone()).as_str(), msg.block_number).await;
                            let mtx = shdstate.read().await;
                            let initialised = mtx.initialised;
                            drop(mtx);
                            if !initialised {
                                log::info!("First stream (= uninitialised). Writing the entire streamed data into the TychoStreamState shared struct.");
                                shd::data::redis::set(keys::stream::status(network.name.clone()).as_str(), SyncState::Syncing as u128).await;
                                // ===== Update Shared State at first sync only =====
                                let mut targets = vec![];
                                for (_id, comp) in msg.new_pairs.iter() {
                                    targets.push(comp.id.to_string().to_lowercase());
                                }
                                let mut mtx = shdstate.write().await;
                                mtx.protosims = msg.states.clone();
                                mtx.components = msg.new_pairs.clone();
                                mtx.initialised = true;
                                log::info!("Shared state updated and dropped");
                                drop(mtx);
                                let mut components = vec![];
                                log::info!("--------- States on network: {} --------- ", network.name);
                                for m in targets.clone() {
                                    if let Some(proto) = msg.states.get(&m.to_string()) {
                                        let comp = msg.new_pairs.get(&m.to_string()).expect("New pair not found");
                                        if comp.id.to_string().contains(NULL_ADDRESS) {
                                            log::info!("Component {} has no address. Skipping.", comp.id);
                                            continue;
                                        }
                                        'outer: for tk in comp.tokens.clone() {
                                            if tk.address.to_string().contains(NULL_ADDRESS) {
                                                break 'outer; // Just skip this component
                                            }
                                        }
                                        let pc = SrzProtocolComponent::from(comp.clone());
                                        components.push(pc.clone());
                                        // match AmmType::from(comp.protocol_type_name.as_str()) {
                                        //     AmmType::Pancakeswap | AmmType::Sushiswap | AmmType::UniswapV2 => {
                                        //         if let Some(_state) = proto.as_any().downcast_ref::<UniswapV2State>() {
                                        //             let pc = SrzProtocolComponent::from(comp.clone());
                                        //             components.push(pc.clone());
                                        //             let key1 = keys::stream::component(network.name.clone(), comp.id.to_string().to_lowercase());
                                        //             shd::data::redis::set(key1.as_str(), pc.clone()).await;
                                        //         } else {
                                        //             log::error!("Downcast to 'UniswapV2State' failed on proto '{}'", comp.protocol_type_name);
                                        //         }
                                        //     }
                                        //     AmmType::UniswapV3 => {
                                        //         if let Some(_state) = proto.as_any().downcast_ref::<UniswapV3State>() {
                                        //             let key1 = keys::stream::component(network.name.clone(), comp.id.to_string().to_lowercase());
                                        //             let pc = SrzProtocolComponent::from(comp.clone());
                                        //             components.push(pc.clone());
                                        //             shd::data::redis::set(key1.as_str(), pc.clone()).await;
                                        //         } else {
                                        //             log::error!("Downcast to 'UniswapV3State' failed on proto '{}'", comp.protocol_type_name);
                                        //         }
                                        //     }
                                        //     AmmType::UniswapV4 => {
                                        //         if let Some(_state) = proto.as_any().downcast_ref::<UniswapV4State>() {
                                        //             let key1 = keys::stream::component(network.name.clone(), comp.id.to_string().to_lowercase());
                                        //             let pc = SrzProtocolComponent::from(comp.clone());
                                        //             components.push(pc.clone());
                                        //             shd::data::redis::set(key1.as_str(), pc.clone()).await;
                                        //         } else {
                                        //             log::error!("Downcast to 'UniswapV4State' failed on proto '{}'", comp.protocol_type_name);
                                        //         }
                                        //     }
                                        //     AmmType::Balancer | AmmType::Curve => {
                                        //         if let Some(_state) = proto.as_any().downcast_ref::<EVMPoolState<PreCachedDB>>() {
                                        //             let key1 = keys::stream::component(network.name.clone(), comp.id.to_string().to_lowercase());
                                        //             let pc = SrzProtocolComponent::from(comp.clone());
                                        //             components.push(pc.clone());
                                        //             shd::data::redis::set(key1.as_str(), pc.clone()).await;
                                        //         } else {
                                        //             log::error!("Downcast to 'EVMPoolState<PreCachedDB>' failed on proto '{}'", comp.protocol_type_name);
                                        //         }
                                        //     }
                                        // }
                                    }
                                }

                                // ===== Storing ALL components =====
                                log::info!("Storing {} components", components.len());
                                let key = keys::stream::components(network.name.clone());
                                shd::data::redis::set(key.as_str(), components.clone()).await;
                                let key = keys::stream::updatedcps(network.name.clone());
                                shd::data::redis::set::<Vec<String>>(key.as_str(), vec![]).await;
                                // ===== Set SyncState to up and running =====
                                shd::data::redis::set(keys::stream::status(network.name.clone()).as_str(), SyncState::Running as u128).await;
                                log::info!("✅ Proto Stream initialised successfully. SyncState set to 'Running' on {}", network.name.clone());
                            } else {
                                // ===== Update Shared State =====
                                // log::info!("Stream already initialised. Updating the mutex-shared state with new data, and updating Redis.");
                                if !msg.states.is_empty() {
                                    log::info!("Received {} new states, updating protosims.", msg.states.len());
                                    let mut mtx = shdstate.write().await;
                                    let cpids = msg.states.keys().map(|x| x.clone().to_lowercase()).collect::<Vec<String>>();
                                    for x in msg.states.iter() {
                                        mtx.protosims.insert(x.0.clone().to_lowercase(), x.1.clone());
                                    }
                                    let key = keys::stream::updatedcps(network.name.clone());
                                    shd::data::redis::set::<Vec<String>>(key.as_str(), cpids.clone()).await;
                                    drop(mtx);
                                }
                                if !msg.new_pairs.is_empty() || !msg.removed_pairs.is_empty() {
                                    log::info!("Received {} new pairs, and {} pairs to be removed. Updating Redis ...", msg.new_pairs.len(), msg.removed_pairs.len());
                                    match api::_components(network.clone()).await {
                                        Some(mut components) => {
                                            for x in msg.new_pairs.iter() {
                                                let pc = SrzProtocolComponent::from(x.1.clone());
                                                if let Some(pos) = components.iter().position(|current| current.id.to_string().to_lowercase() == x.0.to_string().to_lowercase()) {
                                                    components[pos] = pc;
                                                } else {
                                                    components.push(pc);
                                                }
                                            }
                                            for x in msg.removed_pairs.iter() {
                                                if let Some(pos) = components.iter().position(|current| current.id.to_string().to_lowercase() == x.0.to_string().to_lowercase()) {
                                                    components.swap_remove(pos);
                                                }
                                            }
                                            let key = keys::stream::components(network.name.clone());
                                            shd::data::redis::set(key.as_str(), components.clone()).await;
                                        }
                                        None => {
                                            log::error!("Failed to get components. Exiting.");
                                            shd::data::redis::set(keys::stream::status(network.name.clone()).as_str(), SyncState::Error as u128).await;
                                            continue 'retry;
                                        }
                                    }
                                }
                            }
                            // log::info!("--------- Done for {} --------- ", network.name.clone());
                        }
                        Err(e) => {
                            log::info!("🔺 Error: ProtocolStreamBuilder on {}: {:?}. Continuing.", network.name, e);
                            shd::data::redis::set(keys::stream::status(network.name.clone()).as_str(), SyncState::Error as u128).await;
                            // ? --- Set initialised to false --- ?
                            continue;
                        }
                    };
                }
            }
            Err(e) => {
                log::error!("🔺 Failed to create stream: {:?}", e.to_string());
                continue 'retry;
            }
        }
    }
}

/**
 * Stream the entire state from each AMMs, with TychoStreamBuilder.
 */
#[tokio::main]
async fn main() {
    shd::utils::misc::log::new("stream".to_string());
    dotenv::from_filename(".env.prod").ok(); // Use .env.ex for testing
    let config = EnvConfig::new();
    log::info!("Launching Stream on {} | 🧪 Testing mode: {:?}", config.network, config.testing);
    let path = "src/shd/config/networks.json".to_string();
    let networks: Vec<Network> = shd::utils::misc::read(&path);
    let network = networks
        .clone()
        .into_iter()
        .filter(|x| x.enabled)
        .find(|x| x.name == config.network)
        .expect("Network not found or not enabled");
    log::info!("Tycho Stream for '{}' network", network.name.clone());
    shd::data::redis::set(keys::stream::status(network.name.clone()).as_str(), SyncState::Launching as u128).await;
    shd::data::redis::set(keys::stream::stream2(network.name.clone()).as_str(), SyncState::Launching as u128).await;
    shd::data::redis::set(keys::stream::latest(network.name.clone().to_string()).as_str(), 0).await;
    shd::data::redis::ping().await;
    // Shared state
    let stss: SharedTychoStreamState = Arc::new(RwLock::new(TychoStreamState {
        protosims: HashMap::new(),  // Protosims cannot be stored in Redis so we always used shared memory state to access/update them
        components: HashMap::new(), // 📕 Read/write via Redis only
        initialised: false,
    }));
    let readable = Arc::clone(&stss);
    // Start the server, only reading from the shared state
    let dupn = network.clone();
    let dupc = config.clone();
    tokio::spawn(async move {
        loop {
            api::start(dupn.clone(), Arc::clone(&readable), dupc.clone()).await;
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    });
    // Get tokens and launch the stream
    // Start the stream, writing to the shared state
    let writeable = Arc::clone(&stss);
    tokio::spawn(async move {
        loop {
            let config = config.clone();
            let network = network.clone();
            stream(network.clone(), Arc::clone(&writeable), config.clone()).await;
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    });
    futures::future::pending::<()>().await;
    log::info!("Stream program terminated");
}
