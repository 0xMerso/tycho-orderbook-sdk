use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tycho_orderbook::{
    adapters::default::DefaultOrderBookAdapter,
    core::{book, exec::get_original_components, rpc},
    maths::steps::exponential,
    types::{
        ExecutionRequest, OBPEvent, Orderbook, OrderbookBuilder, OrderbookBuilderConfig, OrderbookFunctions, OrderbookProviderConfig, OrderbookRequestParams, SharedTychoStreamState, TychoStreamState,
    },
    utils::r#static::filter::{ADD_TVL_THRESHOLD, REMOVE_TVL_THRESHOLD},
};
use tycho_simulation::tycho_client::feed::component_tracker::ComponentFilter;

#[tokio::main]
async fn main() {
    let filter = tracing_subscriber::EnvFilter::from_default_env(); // Read RUST_tracing env variable
    tracing_subscriber::fmt().with_env_filter(filter).init(); // <--- Set the tracing level here
    tracing::info!("--- --- --- Launching Quickstart Tycho Orderbook --- --- ---");
    // tracing::info!("Gm"); tracing::debug!("Gm"); tracing::trace!("Gm");
    dotenv::from_filename("examples/.env.qs.ex").ok(); // Use .env.ex for testing
    let network_name = std::env::var("NETWORK").expect("Variable 'NETWORK' not found in environment");
    let real_exec = std::env::var("REAL_EXEC").expect("Variable 'REAL_EXEC' not found in environment") == "true";
    let tycho_api_key = std::env::var("TYCHO_API_KEY").expect("Variable 'TYCHO_API_KEY' not found in environment");
    let sender = std::env::var("SENDER").expect("Variable 'SENDER' not found in environment");
    let pk = match std::env::var("PV_KEY") {
        Ok(v) => {
            tracing::info!("Private key found in environment variables");
            Some(v)
        }
        Err(_) => {
            tracing::warn!("Private key not found in environment variables. Continuing without executing any transaction.");
            None
        }
    };
    tracing::info!("Tycho API Key: {}", tycho_api_key);
    tracing::info!("Network: {}", network_name);
    tracing::info!("Sender: {}", sender);
    tracing::info!("Real Execution: {}", real_exec);

    let networks = tycho_orderbook::utils::r#static::networks();
    let network = networks.clone().into_iter().find(|x| x.name == network_name).expect("Network not found");
    tracing::debug!("Tycho Stream for '{}' network", network.name.clone());
    // Create cross/shared state for the protocol stream
    let xstate: SharedTychoStreamState = Arc::new(RwLock::new(TychoStreamState {
        protosims: HashMap::new(),
        components: HashMap::new(),
        initialised: false,
    }));
    let tokens = match rpc::tokens(&network, tycho_api_key.clone()).await {
        Some(t) => t,
        None => {
            tracing::error!("Failed to get tokens. Something anormal, make sure Tycho endpoint is operational. Exiting.");
            return;
        }
    };
    let mut hmt = HashMap::new();
    tokens.iter().for_each(|t| {
        hmt.insert(t.address.clone(), t.clone());
    });

    // --- Adjust as needed --- Mainnet here
    let eth = network.eth.clone().to_lowercase();
    let usdc = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string().to_lowercase(); // base: 0x833589fcd6edb6e08f4c7c32d4f71b54bda02913
    let btc = "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599".to_string().to_lowercase(); // base: 0xcbB7C0000aB88B473b1f5aFd9ef808440eed33Bf
    let btcusdc = format!("{}-{}", btc, usdc); // "0xBTC" "0xUSDC"
    let btc_eth = format!("{}-{}", btc, eth); // "0xBTC" "0xETH"
    let eth_usdc = format!("{}-{}", eth, usdc); // "0xETH" "0xUSDC"
    let mut tracked: HashMap<String, Option<Orderbook>> = HashMap::new();
    // tracked.insert(btcusdc.clone(), None);
    // tracked.insert(btc_eth.clone(), None);
    tracked.insert(eth_usdc.clone(), None);
    let obtag = eth_usdc; // Orderbook tag on which we want to execute a trade for demo
    tracing::debug!("Execution on obtag: {:?}", obtag);

    // Create the OBP provider from the protocol stream builder and shared state.
    let mut attempt = 0;
    let mut executed = false; // Flag to check if the transaction has been executed, to keep one execution only
    let filter = ComponentFilter::with_tvl_range(REMOVE_TVL_THRESHOLD, ADD_TVL_THRESHOLD);
    let builder_config = OrderbookBuilderConfig { filter };
    let provider_config = OrderbookProviderConfig { capacity: 100 };
    let obp = loop {
        attempt += 1;
        let builder = OrderbookBuilder::new(network.clone(), tycho_api_key.clone(), builder_config.clone(), Some(tokens.clone())).await;
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
                    for (key, value) in tracked.clone().iter() {
                        if value.is_none() {
                            let simufns = OrderbookFunctions {
                                optimize: book::optimize,
                                steps: exponential,
                            };
                            tracing::info!("🧱 OBP Event: Orderbook {} isn't build yet, building it ...", key.clone());
                            match obp
                                .get_orderbook(
                                    OrderbookRequestParams {
                                        tag: key.clone().to_lowercase(),
                                        point: None,
                                    },
                                    Some(simufns),
                                )
                                .await
                            {
                                Ok(orderbook) => {
                                    tracing::info!("OBP Event: Orderbook received");
                                    tracked.insert(key.clone().to_lowercase(), Some(orderbook.clone()));
                                }
                                Err(err) => {
                                    tracing::error!("OBP Event: Error: {:?}", err);
                                }
                            }
                        } else {
                            tracing::debug!("OBP Event: Orderbook already built, checking for update.");
                            let current = value.clone().unwrap();
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
                                    steps: exponential,
                                };
                                if let Ok(book) = obp.get_orderbook(OrderbookRequestParams { tag: key.clone(), point: None }, Some(simufns)).await {
                                    let symtag = format!("{}-{}", book.base.symbol, book.quote.symbol);
                                    tracing::info!("OBP Event: Orderbook {} has been updated", symtag);
                                    tracked.insert(key.clone(), Some(book.clone()));

                                    let depth = book.depth(None);
                                    tracing::debug!("Bids ({})", depth.bids.len());
                                    for d in depth.bids {
                                        tracing::trace!(" - {:.5} {} at a price of {:.5} {} per {}", d.1, current.base.symbol, d.0, current.quote.symbol, current.base.symbol);
                                    }
                                    tracing::debug!("Asks ({})", depth.asks.len());
                                    for d in depth.asks {
                                        tracing::trace!(" - {:.5} {} at a price of {:.5} {} per {}", d.1, current.base.symbol, d.0, current.quote.symbol, current.base.symbol);
                                    }

                                    if book.tag.clone().eq_ignore_ascii_case(obtag.as_str()) {
                                        tracing::debug!("OBP Event: Orderbook {} is the one we want to execute a trade on.", symtag);
                                        // Execution
                                        // let amount = book.mpd_base_to_quote.
                                        let way = book.mpd_base_to_quote.clone();
                                        let request = ExecutionRequest {
                                            sender: sender.to_string().clone(),
                                            tag: book.tag.clone(),
                                            input: book.base.clone(),
                                            output: book.quote.clone(),
                                            amount: way.amount,
                                            expected: way.received,
                                            distribution: way.distribution.clone(),
                                            components: book.pools.clone(),
                                        };

                                        let mtx = state.read().await;
                                        let originals = mtx.components.clone();
                                        drop(mtx);
                                        let originals = get_original_components(originals, book.pools.clone());

                                        // match book.create(network.clone(), request, originals.clone(), Some(env.pvkey.clone())).await {
                                        match book.create(network.clone(), request, originals.clone(), None).await {
                                            Ok(payload) => {
                                                if real_exec {
                                                    if !executed {
                                                        let _ = book.send(network.clone(), payload, pk.clone()).await;
                                                        executed = true;
                                                    } else {
                                                        tracing::info!("Tx already executed, not executing again: {}", symtag);
                                                    }
                                                }
                                            }
                                            Err(err) => {
                                                tracing::error!("OBP Event: Error executing orderbook {}: {:?}", symtag, err);
                                            }
                                        }
                                    } else {
                                        tracing::debug!("OBP Event: Orderbook {} is not the one we want to execute a trade on.", symtag);
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
