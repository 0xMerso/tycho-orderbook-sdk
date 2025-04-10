use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tycho_orderbook::{
    adapters::default::DefaultOrderBookAdapter,
    builder::{OrderbookBuilder, OrderbookBuilderConfig},
    core::{client, exec::get_original_components, helper::default_protocol_stream_builder, solver::DefaultOrderbookSolver},
    types::{ExecutionRequest, Orderbook, OrderbookEvent, OrderbookRequestParams, SharedTychoStreamState, TychoStreamState},
    utils::r#static::filter::{ADD_TVL_THRESHOLD, REMOVE_TVL_THRESHOLD},
};
use tycho_simulation::tycho_client::feed::component_tracker::ComponentFilter;

#[tokio::main]
async fn main() {
    let filter = tracing_subscriber::EnvFilter::from_default_env(); // Read RUST_tracing env variable
    tracing_subscriber::fmt().with_env_filter(filter).init(); // <--- Set the tracing level here
    tracing::info!("--- --- --- Launching Quickstart Tycho Orderbook --- --- ---");
    // tracing::info!("Gm"); tracing::debug!("Gm"); tracing::trace!("Gm");
    dotenv::from_filename("examples/.env.quickstart.ex").ok(); // Use .env.ex for testing
    let network_name = std::env::var("NETWORK").expect("Variable 'NETWORK' not found in environment");
    let real_exec = std::env::var("REAL_EXEC").expect("Variable 'REAL_EXEC' not found in environment") == "true";
    let tycho_api_key = std::env::var("TYCHO_API_KEY").expect("Variable 'TYCHO_API_KEY' not found in environment");
    let sender = std::env::var("SENDER").expect("Variable 'SENDER' not found in environment");
    let pk = match std::env::var("PV_KEY") {
        Ok(v) => {
            tracing::info!(" 🔑 Private key found in environment variables");
            Some(v)
        }
        Err(_) => {
            tracing::warn!(" 🔑 Private key not found in environment variables. Continuing without executing any transaction.");
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

    // --- Token list ---
    let tokens = match client::tokens(&network, tycho_api_key.clone()).await {
        Some(t) => t,
        None => {
            tracing::error!("Failed to get tokens. Something anormal, make sure Tycho endpoint is operational. Exiting.");
            return;
        }
    };

    // --- Adjust as needed --- Mainnet
    let eth = network.eth.clone().to_lowercase();
    let (usdc, btc) = match network.name.as_str() {
        "ethereum" => (
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string().to_lowercase(),
            "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599".to_string().to_lowercase(),
        ),
        "base" => (
            "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913".to_string().to_lowercase(),
            "0xcbB7C0000aB88B473b1f5aFd9ef808440eed33Bf".to_string().to_lowercase(),
        ),
        _ => panic!("Network not supported"),
    };
    let mut tracked: HashMap<String, Option<Orderbook>> = HashMap::new();
    let _btcusdc = format!("{}-{}", btc, usdc); // "0xBTC" "0xUSDC"
    let _btceth = format!("{}-{}", btc, eth); // "0xBTC" "0xETH"
    let ethusdc = format!("{}-{}", eth, usdc); // "0xETH" "0xUSDC"
    tracked.insert(ethusdc.clone(), None);
    // tracked.insert(btcusdc.clone(), None);
    // tracked.insert(btceth.clone(), None);

    // --- Quickstart Config --- Target orderbook: ETH-USDC
    let obtag = ethusdc; // Orderbook tag on which we want to execute a trade for demo
    tracing::debug!("Execution on a specific orderbook for quickstart demo: {:?}", obtag);
    let mut executed = false; // Flag to check if the transaction has been executed, to keep one execution only

    // --- Create cross/shared state for the protocol stream ---
    let xstate: SharedTychoStreamState = Arc::new(RwLock::new(TychoStreamState {
        protosims: HashMap::new(),
        components: HashMap::new(),
        initialised: false,
    }));

    // --- Create Protocol stream builder --- Create your own protocol stream builder if you want to custom it.
    let filter = ComponentFilter::with_tvl_range(REMOVE_TVL_THRESHOLD, ADD_TVL_THRESHOLD);
    let psb = default_protocol_stream_builder(network.clone(), tycho_api_key.clone(), OrderbookBuilderConfig { filter }, tokens.clone()).await;

    // --- Create the provider ---
    let builder = OrderbookBuilder::new(network.clone(), psb, tycho_api_key.clone(), tokens.clone());
    match builder.build(xstate).await {
        Ok(provider) => {
            let obp = Arc::new(provider);
            let state = Arc::clone(&obp.state);
            tracing::debug!("OBP Client started. Waiting for updates");
            loop {
                // Arc prevents moving out inner fields, and this loop is creating multiple consumers.
                // Loop indefinitely over the stream, printing received events.
                let mut stream = obp.stream.lock().await;
                if let Some(event) = stream.recv().await {
                    match event {
                        OrderbookEvent::Initialised(block) => {
                            tracing::info!("Event: Initialised: : ✅ Initialised at block {}", block);
                        }
                        OrderbookEvent::NewHeader(block, updated) => {
                            tracing::info!("Event: NewHeader: #{} with {} components updated", block, updated.len());
                            for (key, value) in tracked.clone().iter() {
                                if value.is_none() {
                                    tracing::info!("🧱 OBP Event: Orderbook {} isn't build yet, building it ...", key.clone());
                                    match obp
                                        .get_orderbook(
                                            DefaultOrderbookSolver,
                                            OrderbookRequestParams {
                                                tag: key.clone().to_lowercase(),
                                                point: None,
                                            },
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

                                        if let Ok(book) = obp.get_orderbook(DefaultOrderbookSolver, OrderbookRequestParams { tag: key.clone(), point: None }).await {
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
                                                let way = book.mpd_base_to_quote.clone();
                                                let amount = way.amount / 10.; // By default, the simulation algo provide equivalent amount of 0.01 ETH in base token. So /10 = 0.001 ETH
                                                let expected = way.received / 10.; // Same here
                                                let request = ExecutionRequest {
                                                    sender: sender.to_string().clone(),
                                                    tag: book.tag.clone(),
                                                    input: book.base.clone(),
                                                    output: book.quote.clone(),
                                                    amount,
                                                    expected,
                                                    distribution: way.distribution.clone(),
                                                    components: book.pools.clone(),
                                                };

                                                let mtx = state.read().await;
                                                let originals = mtx.components.clone();
                                                drop(mtx);
                                                let originals = get_original_components(originals, book.pools.clone());

                                                // match book.create(network.clone(), request, originals.clone(), Some(env.pvkey.clone())).await {
                                                match book.create(network.clone(), request, originals.clone(), pk.clone()).await {
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
                        OrderbookEvent::Error(err) => {
                            tracing::error!("OBP Event: Error: {:?}", err);
                        }
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!("Error building Orderbook Provider: {}", e);
            return;
        }
    }
}
