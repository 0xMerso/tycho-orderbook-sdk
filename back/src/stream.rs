use std::collections::HashMap;
use std::str::FromStr;
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
use tap2::shd::types::AmmType;
use tap2::shd::types::EnvConfig;
use tap2::shd::types::Network;
use tap2::shd::types::SharedTychoStreamState;
use tap2::shd::types::SyncState;
use tap2::shd::types::TychoStreamState;
use tap2::shd::types::TychoSupportedProtocol;
use tokio::sync::RwLock;
use tycho_simulation::evm::protocol::filters::curve_pool_filter;
use tycho_simulation::evm::protocol::filters::uniswap_v4_pool_with_hook_filter;
use tycho_simulation::evm::protocol::uniswap_v3::state::UniswapV3State;
use tycho_simulation::evm::protocol::uniswap_v4::state::UniswapV4State;

use tycho_simulation::models::Token;
use tycho_simulation::tycho_core::Bytes;
use tycho_simulation::{
    evm::{
        engine_db::tycho_db::PreCachedDB,
        protocol::{filters::balancer_pool_filter, uniswap_v2::state::UniswapV2State, vm::state::EVMPoolState},
        stream::ProtocolStreamBuilder,
    },
    tycho_client::feed::component_tracker::ComponentFilter,
};

use tycho_simulation::protocol::models::ProtocolComponent;

pub mod api;

/**
 * Stream the entire state from each AMMs, with TychoStreamBuilder.
 */
async fn stream_protocol(network: Network, shdstate: SharedTychoStreamState, tokens: Vec<Token>, config: EnvConfig) {
    log::info!("2️⃣  Launching ProtocolStreamBuilder task for {}", network.name);
    // ===== Tycho Filters =====
    let u4 = uniswap_v4_pool_with_hook_filter;
    let balancer = balancer_pool_filter;
    let curve = curve_pool_filter;
    let (_, _, chain) = shd::types::chain(network.name.clone()).expect("Invalid chain");
    let filter = ComponentFilter::with_tvl_range(1.0, 500.0); // ! Important. 250 ETH minimum

    // ===== Tycho Tokens =====
    let mut hmt = HashMap::new();
    tokens.iter().for_each(|t| {
        hmt.insert(t.address.clone(), t.clone());
    });
    let srztokens = tokens.iter().map(|t| SrzToken::from(t.clone())).collect::<Vec<SrzToken>>();
    let key = keys::stream::tokens(network.name.clone());
    shd::data::redis::set(key.as_str(), srztokens.clone()).await;

    let mut toktag = HashMap::new();

    {
        let weth = hmt.get(&Bytes::from_str(network.eth.as_str()).unwrap()).unwrap_or_else(|| panic!("WETH not found on {}", network.name));
        let usdc = hmt.get(&Bytes::from_str(network.usdc.as_str()).unwrap()).unwrap_or_else(|| panic!("USDC not found on {}", network.name));
        // let wbtc = hmt.get(&Bytes::from_str(network.wbtc.as_str()).unwrap()).unwrap_or_else(|| panic!("WBTC not found on {}", network.name));
        let dai = hmt.get(&Bytes::from_str(network.dai.as_str()).unwrap()).expect("DAI not found");
        let usdt = hmt.get(&Bytes::from_str(network.usdt.as_str()).unwrap()).expect("USDT not found");
        toktag.insert(weth.clone().address, weth.clone());
        toktag.insert(usdc.clone().address, usdc.clone());
        // toktag.insert(wbtc.clone().address, wbtc.clone());
        toktag.insert(dai.clone().address, dai.clone());
        toktag.insert(usdt.clone().address, usdt.clone());
    }

    // ===== Tycho Stream Builder =====
    'retry: loop {
        log::info!("Connecting to >>> ProtocolStreamBuilder <<< at {} on {:?} ...\n", network.tycho, chain);
        let mut psb = ProtocolStreamBuilder::new(&network.tycho, chain)
            .exchange::<UniswapV2State>(TychoSupportedProtocol::UniswapV2.to_string().as_str(), filter.clone(), None) // ! Filter ?
            .exchange::<UniswapV3State>(TychoSupportedProtocol::UniswapV3.to_string().as_str(), filter.clone(), None) // ! Filter ?
            .exchange::<UniswapV4State>(TychoSupportedProtocol::UniswapV4.to_string().as_str(), filter.clone(), Some(u4)) // ! Filter ?
            .auth_key(Some(config.tycho_api_key.clone()))
            .skip_state_decode_failures(true)
            .set_tokens(hmt.clone())
            // block_time - timeout - auth_key - skip_state_decode_failures - set_tokens
            .await;

        if network.name.as_str() == "ethereum" {
            psb = psb
                .exchange::<UniswapV2State>(TychoSupportedProtocol::Sushiswap.to_string().as_str(), filter.clone(), None) // ! Filter ?
                .exchange::<UniswapV2State>(TychoSupportedProtocol::Pancakeswap.to_string().as_str(), filter.clone(), None) // ! Filter ?
                .exchange::<EVMPoolState<PreCachedDB>>(TychoSupportedProtocol::BalancerV2.to_string().as_str(), filter.clone(), Some(balancer))
                .exchange::<EVMPoolState<PreCachedDB>>(TychoSupportedProtocol::Curve.to_string().as_str(), filter.clone(), Some(curve));
        }
        match psb.build().await {
            Ok(mut stream) => {
                // The stream created emits BlockUpdate messages which consist of:
                // - block number- the block this update message refers to
                // - new_pairs- new components witnessed (either recently created or newly meeting filter criteria)
                // - removed_pairs- components no longer tracked (either deleted due to a reorg or no longer meeting filter criteria)
                // - states- the updated ProtocolSimstates for all components modified in this block
                // The first message received will contain states for all protocol components registered to. Thereafter, further block updates will only contain data for updated or new components.
                while let Some(msg) = stream.next().await {
                    match msg {
                        Ok(msg) => {
                            log::info!(
                                "🔸 ProtocolStreamBuilder: received block # {} with {} state, {} new_pairs and {} removed_pairs",
                                msg.block_number,
                                msg.states.len(),
                                msg.new_pairs.len(),
                                msg.removed_pairs.len()
                            );

                            shd::data::redis::set(keys::stream::latest(network.name.clone()).as_str(), msg.block_number).await;

                            let mtx = shdstate.read().await;
                            let initialised = mtx.initialised;
                            drop(mtx);
                            if initialised == false {
                                log::info!("Stream not initialised yet. Waiting for the first message to complete. Setting Redis SyncState");
                                shd::data::redis::set(keys::stream::status(network.name.clone()).as_str(), SyncState::Syncing as u128).await;
                            }

                            // ===== Test Mode Targets (WETH/USDC) =====
                            let mut targets = vec![];
                            let mut pairs: HashMap<String, ProtocolComponent> = HashMap::new();
                            for (id, comp) in msg.new_pairs.iter() {
                                pairs.entry(id.clone()).or_insert_with(|| comp.clone());
                                let t0 = comp.tokens.first().unwrap();
                                let addr0 = t0.address.clone();
                                let t1 = comp.tokens.get(1).unwrap();
                                let addr1 = t1.address.clone();
                                if toktag.contains_key(&addr0) || toktag.contains_key(&addr1) {
                                    targets.push(comp.id.to_string().to_lowercase());
                                }
                            }

                            if !initialised {
                                // ===== Update Shared State at first sync only =====
                                log::info!("First stream (= uninitialised). Writing the entire streamed into the TychoStreamState ArcMutex.");
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
                                        if comp.id.to_string().contains("0x0000000000000000000000000000000000000000") {
                                            log::info!("Component {} has no address. Skipping.", comp.id);
                                            continue;
                                        }
                                        'outer: for tk in comp.tokens.clone() {
                                            if tk.address.to_string().contains("0x0000000000000000000000000000000000000000") {
                                                break 'outer;
                                            }
                                        }
                                        // --- Debug Attributes ---
                                        // let stattribute = comp.static_attributes.clone();
                                        // for (k, v) in stattribute.iter() {
                                        //     let fee = match k == "key_lp_fee" || k == "fee" {
                                        //         true => shd::core::amms::feebps(comp.protocol_type_name.clone(), comp.id.to_string().clone(), v.to_string()),
                                        //         false => 0,
                                        //     };
                                        // }
                                        match AmmType::from(comp.protocol_type_name.as_str()) {
                                            AmmType::Pancakeswap | AmmType::Sushiswap | AmmType::UniswapV2 => {
                                                if let Some(state) = proto.as_any().downcast_ref::<UniswapV2State>() {
                                                    // log::info!("Good downcast to UniswapV2State");
                                                    // log::info!(" - reserve0: {}", state.reserve0.to_string());
                                                    // log::info!(" - reserve1: {}", state.reserve1.to_string());

                                                    let pc = SrzProtocolComponent::from(comp.clone());
                                                    components.push(pc.clone());
                                                    let key1 = keys::stream::component(network.name.clone(), comp.id.to_string().to_lowercase());
                                                    shd::data::redis::set(key1.as_str(), pc.clone()).await;

                                                    let key2 = keys::stream::state(network.name.clone(), comp.id.to_string().to_lowercase());
                                                    let srz = SrzUniswapV2State::from((state.clone(), comp.id.to_string()));
                                                    shd::data::redis::set(key2.as_str(), srz.clone()).await;
                                                } else {
                                                    log::error!("Downcast to 'UniswapV2State' failed on proto '{}'", comp.protocol_type_name);
                                                }
                                            }
                                            AmmType::UniswapV3 => {
                                                if let Some(state) = proto.as_any().downcast_ref::<UniswapV3State>() {
                                                    // log::info!(" - (comp) fee: {:?}", state.fee());
                                                    // log::info!(" - (comp) spot_sprice: {:?}", state.spot_price(base, quote));
                                                    let key1 = keys::stream::component(network.name.clone(), comp.id.to_string().to_lowercase());
                                                    let pc = SrzProtocolComponent::from(comp.clone());
                                                    components.push(pc.clone());
                                                    shd::data::redis::set(key1.as_str(), pc.clone()).await;
                                                    let key2 = keys::stream::state(network.name.clone(), comp.id.to_string().to_lowercase());
                                                    let srz = SrzUniswapV3State::from((state.clone(), comp.id.to_string()));
                                                    shd::data::redis::set(key2.as_str(), srz.clone()).await;
                                                    // log::info!(" - (srz state) liquidity   : {} ", srz.liquidity);
                                                    // log::info!(" - (srz state) sqrt_price  : {} ", srz.sqrt_price.to_string());
                                                    // log::info!(" - (srz state) fee         : {:?} ", srz.fee);
                                                    // log::info!(" - (srz state) tick        : {} ", srz.tick);
                                                    // log::info!(" - (srz state) tick_spacing: {} ", srz.ticks.tick_spacing);
                                                    // log::info!(" - (srz state) ticks len   : {}", srz.ticks.ticks.len());
                                                } else {
                                                    log::error!("Downcast to 'UniswapV3State' failed on proto '{}'", comp.protocol_type_name);
                                                }
                                            }
                                            AmmType::UniswapV4 => {
                                                if let Some(state) = proto.as_any().downcast_ref::<UniswapV4State>() {
                                                    let key1 = keys::stream::component(network.name.clone(), comp.id.to_string().to_lowercase());
                                                    let pc = SrzProtocolComponent::from(comp.clone());
                                                    components.push(pc.clone());
                                                    shd::data::redis::set(key1.as_str(), pc.clone()).await;
                                                    let key2 = keys::stream::state(network.name.clone(), comp.id.to_string().to_lowercase());
                                                    let srz = SrzUniswapV4State::from((state.clone(), comp.id.to_string()));
                                                    shd::data::redis::set(key2.as_str(), srz.clone()).await;
                                                    // log::info!(" - (srz state) liquidity   : {} ", srz.liquidity);
                                                    // log::info!(" - (srz state) sqrt_price  : {:?} ", srz.sqrt_price);
                                                    // log::info!(" - (srz state) tick        : {} ", srz.tick);
                                                    // log::info!(" - (srz state) tick_spacing: {} ", srz.ticks.tick_spacing);
                                                    // log::info!(" - (srz state) ticks len   : {} ", srz.ticks.ticks.len());
                                                } else {
                                                    log::error!("Downcast to 'UniswapV4State' failed on proto '{}'", comp.protocol_type_name);
                                                }
                                            }
                                            AmmType::Balancer | AmmType::Curve => {
                                                if let Some(state) = proto.as_any().downcast_ref::<EVMPoolState<PreCachedDB>>() {
                                                    let key1 = keys::stream::component(network.name.clone(), comp.id.to_string().to_lowercase());
                                                    let pc = SrzProtocolComponent::from(comp.clone());
                                                    components.push(pc.clone());
                                                    shd::data::redis::set(key1.as_str(), pc.clone()).await;
                                                    let key2 = keys::stream::state(network.name.clone(), comp.id.to_string().to_lowercase());
                                                    let srz = SrzEVMPoolState::from((state.clone(), comp.id.to_string()));
                                                    // log::info!(" - spot_sprice: {:?}", state.spot_price(base, quote));
                                                    // log::info!(" - (srz state) id        : {} ", srz.id);
                                                    // log::info!(" - (srz state) tokens    : {:?} ", srz.tokens);
                                                    // log::info!(" - (srz state) block     : {} ", srz.block);
                                                    // log::info!(" - (srz state) balances  : {:?} ", srz.balances);
                                                    shd::data::redis::set(key2.as_str(), srz.clone()).await;
                                                } else {
                                                    log::error!("Downcast to 'EVMPoolState<PreCachedDB>' failed on proto '{}'", comp.protocol_type_name);
                                                }
                                            }
                                        }
                                    }
                                }

                                // ===== Storing ALL components =====
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
    let network = networks.clone().into_iter().filter(|x| x.enabled).find(|x| x.name == config.network).expect("Network not found or not enabled");
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
    match shd::core::client::tokens(&network, &config).await {
        Some(tokens) => {
            // Start the stream, writing to the shared state
            let writeable = Arc::clone(&stss);
            tokio::spawn(async move {
                loop {
                    let config = config.clone();
                    let network = network.clone();
                    stream_protocol(network.clone(), Arc::clone(&writeable), tokens.clone(), config.clone()).await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            });
        }
        None => {
            log::error!("Failed to get tokens. Exiting.");
        }
    }
    futures::future::pending::<()>().await;
    log::info!("Stream program terminated");
}
