use std::{collections::HashMap, sync::Arc};
use tycho_orderbook::{
    adapters::default::DefaultOrderBookAdapter,
    builder::OrderbookBuilder,
    core::{client, helper::get_original_components, solver::DefaultOrderbookSolver},
    types::{ExecutionRequest, Orderbook, OrderbookEvent, OrderbookRequestParams},
};

/// Quickstart example for Tycho Orderbook
/// This example demonstrates how to use the Tycho Orderbook library to create an orderbook provider and execute trades on it.
/// It open a Orderbook stream and listen for events, under the hood, it uses the Tycho protocol stream.
/// If the env variable REAL_EXEC is set to true, it will execute the trades on the network, with the private key provided in the env variable PV_KEY.
/// Here, the swap executed is the best bid/ask on the orderbook, it will be worth less than 0.001 ETH (max)

#[tokio::main]
async fn main() {
    let filter = tracing_subscriber::EnvFilter::from_default_env(); // Read RUST_tracing env variable
    tracing_subscriber::fmt().with_env_filter(filter).init(); // <--- Set the tracing level here
    tracing::info!("--- --- --- Launching Quickstart Tycho Orderbook --- --- ---");

    // --- Load environment variables ---
    //  The env file is expected to be in examples/ folder, nott in the root folder.
    dotenv::from_filename("examples/.env.quickstart.ex").ok();
    let network_name = std::env::var("NETWORK").expect("Variable 'NETWORK' not found in environment");
    let real_exec = std::env::var("REAL_EXEC").expect("Variable 'REAL_EXEC' not found in environment") == "true";
    let tychokey = std::env::var("TYCHO_API_KEY").expect("Variable 'TYCHO_API_KEY' not found in environment");
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
    tracing::info!("Tycho API Key: {}", tychokey);
    tracing::info!("Network: {}", network_name);
    tracing::info!("Sender: {}", sender);
    tracing::info!("Real Execution: {}", real_exec);
    let networks = tycho_orderbook::utils::r#static::networks();
    let network = networks.clone().into_iter().find(|x| x.name == network_name).expect("Network not found");
    tracing::debug!("Tycho Stream for '{}' network", network.name.clone());

    // --- Token list ---
    let tokens = match client::tokens(&network, tychokey.clone()).await {
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

    // --- Create the provider ---
    let obb = OrderbookBuilder::new(network.clone(), None, tychokey.clone(), tokens.clone()).await;
    match obb.build().await {
        Ok(provider) => {
            let obp = Arc::new(provider);
            let state = Arc::clone(&obp.state);
            tracing::debug!("OrderbookProvider built. Waiting for updates");
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
                                match value {
                                    Some(current) => {
                                        tracing::debug!("OBP Event: Orderbook already built, checking for update.");
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

                                                    tracing::info!("Creating the transactions to execute ...");
                                                    // match book.create(network.clone(), request, originals.clone(), Some(env.pvkey.clone())).await {
                                                    match book.create(network.clone(), request, originals.clone(), pk.clone()).await {
                                                        Ok(payload) => {
                                                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await; // Wait a bit before executing the transaction, to check the logs.
                                                            if real_exec {
                                                                if !executed {
                                                                    match book.send(network.clone(), payload, pk.clone()).await {
                                                                        Ok(_executed_payload) => {
                                                                            tracing::info!("Orderbook {} : Executed successfully", symtag);
                                                                            executed = true;
                                                                        }
                                                                        Err(err) => {
                                                                            tracing::error!("Error executing orderbook {}: {:?}", symtag, err);
                                                                        }
                                                                    }
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
                                    None => {
                                        tracing::info!("🧱 OBP Event: Orderbook {} isn't build yet, building it ...", key.clone());
                                        match obp
                                            .get_orderbook(
                                                DefaultOrderbookSolver,
                                                OrderbookRequestParams {
                                                    tag: key.clone().to_lowercase(),
                                                    point: None, // If you just need 1 point on the orderbook
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
                                    }
                                }
                            }
                            let mtx = state.read().await;
                            let initialised = mtx.initialised;
                            let cps = mtx.components.len();
                            let pts = mtx.protosims.len();
                            drop(mtx);
                            tracing::info!("OBP Event: Shared state initialised status: {} | Comp size: {} | Pts size: {}", initialised, cps, pts);
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
