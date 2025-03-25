
// if let Some(cpbs) = balances.get(&pdata.component.id.to_lowercase()) {
        //     let t0b = cpbs.get(&srzt0.address.to_lowercase()).unwrap_or(&0u128);
        //     log::info!(" t0b => {}", *t0b as f64 / 10f64.powi(srzt0.decimals as i32));
        //     let t1b = cpbs.get(&srzt1.address.to_lowercase()).unwrap_or(&0u128);
        //     log::info!(" t1b => {}", *t1b as f64 / 10f64.powi(srzt1.decimals as i32));
        // }

// Main

    // Start the server, only reading from the shared state
    let dupn = network.clone();
    let dupc = config.clone();
    let writeable = Arc::clone(&stss);
    tokio::spawn(async move {
        loop {
            stream_state(dupn.clone(), Arc::clone(&writeable), dupc.clone()).await;
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    });

    shd::data::redis::wstatus(keys::stream::stream2(network.name.clone()).to_string(), "TychoStream thread to be initialized".to_string()).await;



/**
 * Stream a part of state from each components, with TychoStreamBuilder.
 * Mostly used to get components balances
 * Didn't know at the time that HttpClient could be used to fetch states/balances, instead of opening a stream here. Can be usefull later so keep it.
 */
async fn stream_state(network: Network, shared: SharedTychoStreamState, config: EnvConfig) {
    let (_, chain, _) = shd::types::chain(network.name.clone()).unwrap();
    log::info!("1️⃣  Launching TychoStreamBuilder task for {} | Endpoint: {} | Chain {}", network.name, network.tycho, chain);
    let filter = ComponentFilter::with_tvl_range(1.0, 50.0);

    let mut tsb = tycho_simulation::tycho_client::stream::TychoStreamBuilder::new(&network.tycho, chain)
        .exchange(TychoSupportedProtocol::UniswapV2.to_string().as_str(), filter.clone())
        .exchange(TychoSupportedProtocol::UniswapV3.to_string().as_str(), filter.clone())
        .exchange(TychoSupportedProtocol::UniswapV4.to_string().as_str(), filter.clone())
        .auth_key(Some(config.tycho_api_key.clone()));
    // .block_time(shd::types::chain_timing(network.name.clone()) * 10); // No need to update every block // ? Seems it has no effect !

    // let timing = tsb.block_time(block_time)

    if network.name.as_str() == "ethereum" {
        tsb = tsb
            .exchange(TychoSupportedProtocol::Sushiswap.to_string().as_str(), filter.clone())
            .exchange(TychoSupportedProtocol::Pancakeswap.to_string().as_str(), filter.clone())
            .exchange(TychoSupportedProtocol::BalancerV2.to_string().as_str(), filter.clone())
            .exchange(TychoSupportedProtocol::Curve.to_string().as_str(), filter.clone());
    }

    match tsb.build().await {
        Ok((mut _handle, mut receiver)) => {
            while let Some(fm) = receiver.recv().await {
                log::info!("🔹 TychoStreamBuilder [for balances only]: received {} state messages and {} sync states", fm.state_msgs.len(), fm.sync_states.len());
                let mtx = shared.read().await;
                let mut updated = mtx.balances.clone();
                let _before = updated.len();
                drop(mtx);
                let before = updated.len();
                let amms = TychoSupportedProtocol::vectorize();
                for amm in amms.clone() {
                    match fm.state_msgs.get(amm.as_str()) {
                        Some(msg) => {
                            let snapshots = msg.snapshots.get_states().clone();
                            if !snapshots.is_empty() {
                                // log::info!("AMM: {} | Got {} state messages on block {}", amm, snapshots.len(), msg.header.number);
                                for s in snapshots.iter() {
                                    let cws = s.1.clone();
                                    let state = cws.state;
                                    let component = cws.component;
                                    let mut fmt = HashMap::new();
                                    let tmp = state.balances.clone();
                                    for (token, balance) in tmp.iter() {
                                        let balance = balance.to_string();
                                        let balance = balance.trim_start_matches("0x");
                                        let value = u128::from_str_radix(balance, 16).unwrap();
                                        fmt.insert(token.to_string().to_lowercase(), value);
                                    }
                                    updated.insert(component.id.clone().to_string().to_lowercase(), fmt.clone());
                                }
                            }
                        }
                        None => {
                            // log::info!("No state message for {}", amm);
                            // shd::data::redis::set(keys::stream::stream2(network.name.clone()).as_str(), SyncState::Error as u128).await;
                        }
                    }
                }
                if _before == 0 {
                    log::info!("✅ TychoStreamBuilder [balances] ready and synced.");
                }
                let after = updated.len();
                let mut mtx = shared.write().await;
                mtx.balances = updated.clone();
                drop(mtx);
                if after > before {
                    // log::info!("Setting stream2 status to 'Running'");
                    shd::data::redis::set(keys::stream::stream2(network.name.clone()).as_str(), SyncState::Running as u128).await;
                    log::info!("Shared balances hashmap updated. Currently {} entries in memory (before = {})", after, before);
                    let path = format!("misc/data-back/{}.stream-balances.json", network.name);
                    crate::shd::utils::misc::save1(updated.clone(), path.as_str());
                }
            }
        } // Failed to build tycho stream: BlockSynchronizerError("Not a single synchronizer healthy!")
        Err(e) => {
            log::error!("Failed to create stream: {:?}", e.to_string());
            // shd::data::redis::set(keys::stream::stream2(network.name.clone()).as_str(), SyncState::Error as u128).await;
        }
    }
}
