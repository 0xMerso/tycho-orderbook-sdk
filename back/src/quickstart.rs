use std::str::FromStr;
use std::{collections::HashMap, sync::Arc};
use tap2::shd::data::fmt::SrzToken;
use tap2::shd::types::{EnvConfig, OBPConfig, OBPEvent, Orderbook, OrderbookBuilder, OrderbookFunctions, OrderbookRequestParams, SharedTychoStreamState, TychoStreamState};
use tokio::sync::RwLock;

use tap2::shd;
use tap2::shd::types::Network;
use tycho_simulation::tycho_core::Bytes;

#[tokio::main]
async fn main() {
    shd::utils::misc::log::new("obpc".to_string());
    dotenv::from_filename(".env.prod").ok(); // Use .env.ex for testing
    let env = EnvConfig::new();
    log::info!("Launching OBP Client on {} | 🧪 Testing mode: {:?}", env.network, env.testing);
    let path = "src/shd/config/networks.json".to_string();
    let networks: Vec<Network> = shd::utils::misc::read(&path);
    let network = networks
        .clone()
        .into_iter()
        .filter(|x| x.enabled)
        .find(|x| x.name == env.network)
        .expect("Network not found or not enabled");
    log::info!("Tycho Stream for '{}' network", network.name.clone());
    // Create cross/shared state for the protocol stream
    let xstate: SharedTychoStreamState = Arc::new(RwLock::new(TychoStreamState {
        protosims: HashMap::new(),  // Customize with your actual types
        components: HashMap::new(), // Customize with your actual types
        initialised: false,
    }));

    // --- Testing|Demo ---
    let tokens = shd::core::rpc::tokens(&network, &env).await.unwrap();
    let mut hmt = HashMap::new();
    tokens.iter().for_each(|t| {
        hmt.insert(t.address.clone(), t.clone());
    });
    let weth = SrzToken::from(
        hmt.get(&Bytes::from_str(network.eth.as_str()).unwrap())
            .unwrap_or_else(|| panic!("WETH not found on {}", network.name))
            .clone(),
    );
    let usdc = SrzToken::from(
        hmt.get(&Bytes::from_str(network.usdc.as_str()).unwrap())
            .unwrap_or_else(|| panic!("USDC not found on {}", network.name))
            .clone(),
    );
    let wbtc = SrzToken::from(
        hmt.get(&Bytes::from_str(network.wbtc.as_str()).unwrap())
            .unwrap_or_else(|| panic!("WBTC not found on {}", network.name))
            .clone(),
    );
    let mut tracked: HashMap<String, Option<Orderbook>> = HashMap::new();
    // tracked.insert(format!("{}-{}", weth.address.clone().to_lowercase(), usdc.address.clone().to_lowercase()), None);
    tracked.insert(format!("{}-{}", usdc.address.clone().to_lowercase(), wbtc.address.clone().to_lowercase()), None);
    tracked.insert(format!("{}-{}", weth.address.clone().to_lowercase(), wbtc.address.clone().to_lowercase()), None);
    // --- --- --- --- ---
    // Create the OBP provider from the protocol stream builder and shared state.
    let builder = OrderbookBuilder::new(network.clone(), env.clone(), Some(tokens.clone())).await;
    let config = OBPConfig { capacity: 100 };
    let mut _obp = builder.build(config.clone(), xstate).await.expect("Failed to build OBP. Retry or check logs");
    let obp = Arc::new(_obp);
    let state = Arc::clone(&obp.state);

    log::info!("OBP Client started. Waiting for updates");
    loop {
        // Loop indefinitely over the stream, printing received events.
        let mut locked = obp.stream.lock().await;
        if let Some(event) = locked.recv().await {
            match event {
                OBPEvent::Initialised(block) => {
                    log::info!("Event: Initialised: : ✅ Initialised at block {}", block);
                }
                OBPEvent::NewHeader(block, updated) => {
                    log::info!("Event: NewHeader: ⛏️ #{} with {} components updated", block, updated.len());

                    // First
                    for (k, v) in tracked.clone().iter() {
                        if v.is_none() {
                            let simufns = OrderbookFunctions {
                                optimize: shd::core::book::optimize_fast,
                            };
                            log::info!("OBP Event: Orderbook {} isn't build yet, building it ...", k.clone());
                            match obp.get_orderbook(OrderbookRequestParams { tag: k.clone(), sps: None }, Some(simufns)).await {
                                Ok(orderbook) => {
                                    log::info!("OBP Event: Orderbook received");
                                    tracked.insert(k.clone(), Some(orderbook.clone()));
                                }
                                Err(err) => {
                                    log::error!("OBP Event: Error: {:?}", err);
                                }
                            }
                        } else {
                            log::info!("OBP Event: Orderbook already built, checking for update.");
                            let current = v.clone().unwrap();
                            let cps = current.pools.clone();
                            // If one of the components/pools is updated, we need to update the orderbook too.
                            let mut refresh = false;
                            for (x, cp) in cps.iter().enumerate() {
                                if updated.contains(&cp.id.to_lowercase()) {
                                    log::info!(
                                        "- Component #{x} {} {} for {}-{} orderbook has changed, need to update it",
                                        cp.id,
                                        cp.protocol_type_name,
                                        current.base.symbol,
                                        current.quote.symbol
                                    );
                                    refresh = true;
                                }
                            }
                            if refresh {
                                log::info!(" ⚖️ Orderbook {}-{} has changed, need to update it", current.base.symbol, current.quote.symbol);
                                let simufns = OrderbookFunctions {
                                    optimize: shd::core::book::optimize_fast,
                                };
                                if let Ok(newob) = obp.get_orderbook(OrderbookRequestParams { tag: k.clone(), sps: None }, Some(simufns)).await {
                                    log::info!("OBP Event: Orderbook updated");
                                    tracked.insert(k.clone(), Some(newob));
                                } else {
                                    log::error!("OBP Event: Error updating orderbook");
                                }
                            } else {
                                log::info!("Orderbook {}-{} hasn't changed, no need to update it", current.base.symbol, current.quote.symbol);
                            }
                        }
                    }
                    let mtx = state.read().await;
                    let initialised = mtx.initialised;
                    let cps = mtx.components.len();
                    let pts = mtx.protosims.len();
                    drop(mtx);
                    log::info!("OBP Event: Shared state initialised status: {} | Comp size: {} | Pts size: {}", initialised, cps, pts);

                    // --- Testing|Demo ---
                    // let params = obp.generate_random_orderbook_params(1).await;
                    // match obp.get_orderbook(params.clone()).await {
                    //     Ok(orderbook) => {
                    //         log::info!("OBP Event: Orderbook received");
                    //         let path = format!("misc/data-obpc/{}.json", params.tag.clone());
                    //         crate::shd::utils::misc::save1(orderbook.clone(), path.as_str());
                    //     }
                    //     Err(err) => {
                    //         log::error!("OBP Event: Error: {:?}", err);
                    //     }
                    // };
                    // --- --- --- --- ---
                }
                OBPEvent::Error(err) => {
                    log::error!("OBP Event: Error: {:?}", err);
                } // OBPEvent : OrderbookBuilt(tag)
                  // OBPEvent : OrderbookUdapted(tag)
            }
        }
    }
}
