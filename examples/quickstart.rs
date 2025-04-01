use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tycho_client::feed::component_tracker::ComponentFilter;
use tycho_orderbook::{
    adapter::OrderBookAdapter,
    core::{book, rpc},
    types::{
        EnvConfig, OBPEvent, Orderbook, OrderbookBuilder, OrderbookBuilderConfig, OrderbookFunctions, OrderbookProviderConfig, OrderbookRequestParams, SharedTychoStreamState,
        TychoStreamState,
    },
    utils::{
        r#static::filter::{ADD_TVL_THRESHOLD, REMOVE_TVL_THRESHOLD},
    },
};

#[tokio::main]
async fn main() {
    let filter = tracing_subscriber::EnvFilter::from_default_env(); // Read RUST_tracing env variable
    tracing_subscriber::fmt().with_env_filter(filter).init(); // <--- Set the tracing level here
    tracing::info!("--- --- --- Launching Quickstart Tycho Orderbook --- --- ---");
    // tracing::info!("Gm"); tracing::debug!("Gm"); tracing::trace!("Gm");
    dotenv::from_filename(".env.ex").ok(); // Use .env.ex for testing
    let env = EnvConfig::new();
    let networks = tycho_orderbook::utils::r#static::networks();
    let network = networks.clone().into_iter().find(|x| x.name == env.network).expect("Network not found");
    tracing::debug!("Tycho Stream for '{}' network", network.name.clone());
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
    let eth = network.eth.clone();
    let usdc = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string().to_lowercase(); // base: 0x833589fcd6edb6e08f4c7c32d4f71b54bda02913
    let btc = "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599".to_string().to_lowercase(); // base: 0xcbB7C0000aB88B473b1f5aFd9ef808440eed33Bf
    let btcusdc = format!("{}-{}", btc, usdc); // "BTC" "USDC"
    let btc_eth = format!("{}-{}", btc, eth); // "BTC" "ETH"
    let eth_usdc = format!("{}-{}", eth, usdc); // "ETH" "USDC"
    let mut tracked: HashMap<String, Option<Orderbook>> = HashMap::new();
    tracked.insert(btcusdc, None);
    tracked.insert(btc_eth, None);
    tracked.insert(eth_usdc, None);
    // --- --- --- --- ---

    // Create the OBP provider from the protocol stream builder and shared state.
    let mut attempt = 0;
    let filter = ComponentFilter::with_tvl_range(REMOVE_TVL_THRESHOLD, ADD_TVL_THRESHOLD);
    let builder_config = OrderbookBuilderConfig { filter };
    let provider_config = OrderbookProviderConfig { capacity: 100 };
    let obp = loop {
        attempt += 1;
        let builder = OrderbookBuilder::new(network.clone(), env.clone(), builder_config.clone(), Some(tokens.clone())).await;
        match builder.build(provider_config.clone(), xstate.clone()).await {
            Ok(obp) => {
                tracing::info!("Successfully built OBP after {} attempts", attempt);
                break obp;
            }
            Err(err) => {
                tracing::error!("Attempt {}: Failed to build Orderbook Provider: {}. Retrying ...", attempt, err);
                tokio::time::sleep(tokio::time::Duration::from_millis(2500)).await;
            }
        }
    };

    let obp = Arc::new(obp);
    let state = Arc::clone(&obp.state);
    tracing::debug!("OBP Client started. Waiting for updates");
    loop {
        // Loop indefinitely over the stream, printing received events.
        let mut locked = obp.stream.lock().await;
        if let Some(event) = locked.recv().await {
            match event {
                OBPEvent::Initialised(block) => {
                    tracing::info!("Event: Initialised: : ✅ Initialised at block {}", block);
                }
                OBPEvent::NewHeader(block, updated) => {
                    tracing::info!("Event: NewHeader: #{} with {} components updated", block, updated.len());
                    for (k, v) in tracked.clone().iter() {
                        if v.is_none() {
                            let simufns = OrderbookFunctions {
                                optimize: book::optimize,
                                steps: book::steps,
                            };
                            tracing::info!("🧱 OBP Event: Orderbook {} isn't build yet, building it ...", k.clone());
                            match obp.get_orderbook(OrderbookRequestParams { tag: k.clone(), sps: None }, Some(simufns)).await {
                                Ok(orderbook) => {
                                    tracing::info!("OBP Event: Orderbook received");
                                    tracked.insert(k.clone(), Some(orderbook.clone()));
                                }
                                Err(err) => {
                                    tracing::error!("OBP Event: Error: {:?}", err);
                                }
                            }
                        } else {
                            tracing::debug!("OBP Event: Orderbook already built, checking for update.");
                            let current = v.clone().unwrap();
                            let cps = current.pools.clone();
                            // If one of the components/pools is updated, we need to update the orderbook too.
                            let mut refresh = false;
                            for (x, cp) in cps.iter().enumerate() {
                                if updated.contains(&cp.id.to_lowercase()) {
                                    tracing::info!(
                                        " - Component #{x} {} {} for {}-{} orderbook has changed, need to update it",
                                        cp.id,
                                        cp.protocol_type_name,
                                        current.base.symbol,
                                        current.quote.symbol
                                    );
                                    refresh = true;
                                }
                            }
                            if refresh {
                                tracing::info!(" ⚖️  Orderbook {}-{} has changed, need to update it", current.base.symbol, current.quote.symbol);
                                let simufns = OrderbookFunctions {
                                    optimize: book::optimize,
                                    steps: book::steps,
                                };
                                if let Ok(newob) = obp.get_orderbook(OrderbookRequestParams { tag: k.clone(), sps: None }, Some(simufns)).await {
                                    tracing::info!("OBP Event: Orderbook {}-{} has been updated", current.base.symbol, current.quote.symbol);
                                    tracked.insert(k.clone(), Some(newob.clone()));

                                    let depth = newob.depth(None);
                                    tracing::info!("Bids ({})", depth.bids.len());
                                    for d in depth.bids {
                                        tracing::info!(" - {:.5} {} at a price of {:.5} {} per {}", d.1, current.base.symbol, d.0, current.quote.symbol, current.base.symbol);
                                    }
                                    tracing::info!("Asks ({})", depth.asks.len());
                                    for d in depth.asks {
                                        tracing::info!(" - {:.5} {} at a price of {:.5} {} per {}", d.1, current.base.symbol, d.0, current.quote.symbol, current.base.symbol);
                                    }
                                } else {
                                    tracing::error!("OBP Event: Error updating orderbook");
                                }
                            } else {
                                tracing::info!("Orderbook {}-{} hasn't changed, no need to update it", current.base.symbol, current.quote.symbol);
                            }
                        }
                    }
                    let mtx = state.read().await;
                    let initialised = mtx.initialised;
                    let cps = mtx.components.len();
                    let pts = mtx.protosims.len();
                    drop(mtx);
                    tracing::info!("OBP Event: Shared state initialised status: {} | Comp size: {} | Pts size: {}", initialised, cps, pts);

                    // --- Testing|Demo ---
                    // let params = obp.generate_random_orderbook_params(1).await;
                    // match obp.get_orderbook(params.clone()).await {
                    //     Ok(orderbook) => {
                    //         tracing::info!("OBP Event: Orderbook received");
                    //         let path = format!("misc/data-obpc/{}.json", params.tag.clone());
                    //         crate::shd::utils::misc::save1(orderbook.clone(), path.as_str());
                    //     }
                    //     Err(err) => {
                    //         tracing::error!("OBP Event: Error: {:?}", err);
                    //     }
                    // };
                    // --- --- --- --- ---
                }
                OBPEvent::Error(err) => {
                    tracing::error!("OBP Event: Error: {:?}", err);
                } // OBPEvent : OrderbookBuilt(tag)
                  // OBPEvent : OrderbookUdapted(tag)
            }
        }
    }
}
