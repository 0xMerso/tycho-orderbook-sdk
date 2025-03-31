use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tycho_orderbook::{
    adapter::OrderBookAdapter,
    core::{book, rpc},
    types::{EnvConfig, Network, OBPConfig, OBPEvent, Orderbook, OrderbookBuilder, OrderbookFunctions, OrderbookRequestParams, SharedTychoStreamState, TychoStreamState},
    utils,
};

#[tokio::main]
async fn main() {
    dotenv::from_filename(".env.prod").ok(); // Use .env.ex for testing
    let env = EnvConfig::new();
    // log::info!("Launching OBP Client on {} | 🧪 Testing mode: {:?}", env.network, env.testing);
    let path = "networks.json".to_string();
    let networks: Vec<Network> = utils::misc::read(&path);
    let network = networks.clone().into_iter().find(|x| x.name == env.network).expect("Network not found or not enabled");
    log::info!("Tycho Stream for '{}' network", network.name.clone());
    // Create cross/shared state for the protocol stream
    let xstate: SharedTychoStreamState = Arc::new(RwLock::new(TychoStreamState {
        protosims: HashMap::new(),
        components: HashMap::new(),
        initialised: false,
    }));

    let tokens = rpc::tokens(&network, &env).await.unwrap();
    let mut hmt = HashMap::new();
    tokens.iter().for_each(|t| {
        hmt.insert(t.address.clone(), t.clone());
    });

    // --- Adjust as needed --- Mainnet here
    let eth = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
    let usdc = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
    let btc = "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599";
    let btcusdc = format!("{}-{}", btc, usdc); // "BTC" "USDC"
    let btc_eth = format!("{}-{}", btc, eth); // "BTC" "ETH"
    let eth_usdc = format!("{}-{}", eth, usdc); // "ETH" "USDC"
    let mut tracked: HashMap<String, Option<Orderbook>> = HashMap::new();
    tracked.insert(btcusdc, None);
    tracked.insert(btc_eth, None);
    tracked.insert(eth_usdc, None);
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
                    log::info!("Event: NewHeader: #{} with {} components updated", block, updated.len());

                    // First
                    for (k, v) in tracked.clone().iter() {
                        if v.is_none() {
                            let simufns = OrderbookFunctions { optimize: book::optifast };
                            log::info!("🧱 OBP Event: Orderbook {} isn't build yet, building it ...", k.clone());
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
                                        "- 📍 Component #{x} {} {} for {}-{} orderbook has changed, need to update it",
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
                                let simufns = OrderbookFunctions { optimize: book::optifast };
                                if let Ok(newob) = obp.get_orderbook(OrderbookRequestParams { tag: k.clone(), sps: None }, Some(simufns)).await {
                                    log::info!("OBP Event: Orderbook updated");
                                    tracked.insert(k.clone(), Some(newob.clone()));

                                    let depth = newob.depth(None);
                                    for d in depth.bids {
                                        log::info!("bids: {:?}", d);
                                    }
                                    for d in depth.asks {
                                        log::info!("asks: {:?}", d);
                                    }
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
