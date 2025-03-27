use futures::StreamExt;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tycho_simulation::models::Token;
use tycho_simulation::tycho_client::stream::StreamError;

use tycho_simulation::evm::protocol::filters::curve_pool_filter;
use tycho_simulation::evm::protocol::filters::uniswap_v4_pool_with_hook_filter;
use tycho_simulation::evm::protocol::uniswap_v3::state::UniswapV3State;
use tycho_simulation::evm::protocol::uniswap_v4::state::UniswapV4State;

use tycho_simulation::{
    evm::{
        engine_db::tycho_db::PreCachedDB,
        protocol::{filters::balancer_pool_filter, uniswap_v2::state::UniswapV2State, vm::state::EVMPoolState},
        stream::ProtocolStreamBuilder,
    },
    tycho_client::feed::component_tracker::ComponentFilter,
};

use crate::shd;
use crate::shd::r#static::filter::ADD_TVL_THRESHOLD;
use crate::shd::r#static::filter::REMOVE_TVL_THRESHOLD;
use crate::shd::types::{OBPEvent, OrderbookProvider, TychoSupportedProtocol};

use super::data::fmt::SrzProtocolComponent;
use super::data::fmt::SrzToken;
use super::types::{EnvConfig, Network, SharedTychoStreamState};
use super::types::{OBPConfig, OrderbookRequestParams};
use super::types::{Orderbook, OrderbookBuilder};
use super::types::{OrderbookFunctions, ProtoTychoState};

impl OrderbookBuilder {
    /**
     * Default logic to create a ProtocolStreamBuilder, used to build a OrderbookProvider
     * For more advanced use-cases, you can create your own ProtocolStreamBuilder and pass it to custom() fn
     */
    pub async fn new(network: Network, config: EnvConfig, tokens: Option<Vec<Token>>) -> Self {
        let (_, _, chain) = shd::types::chain(network.name.clone()).expect("Invalid chain");
        let u4 = uniswap_v4_pool_with_hook_filter;
        let balancer = balancer_pool_filter;
        let curve = curve_pool_filter;
        let filter = ComponentFilter::with_tvl_range(REMOVE_TVL_THRESHOLD, ADD_TVL_THRESHOLD);
        let tokens = match tokens {
            Some(t) => t,
            None => shd::core::client::tokens(&network, &config).await.unwrap(),
        };
        let mut hmt = HashMap::new();
        let mut srzt = vec![];
        tokens.iter().for_each(|t| {
            hmt.insert(t.address.clone(), t.clone());
            srzt.push(SrzToken::from(t.clone()));
        });
        log::info!("Prebuild. Got {} tokens", hmt.len());
        let mut psb = ProtocolStreamBuilder::new(&network.tycho, chain)
            .exchange::<UniswapV2State>(TychoSupportedProtocol::UniswapV2.to_string().as_str(), filter.clone(), None)
            .exchange::<UniswapV3State>(TychoSupportedProtocol::UniswapV3.to_string().as_str(), filter.clone(), None)
            .exchange::<UniswapV4State>(TychoSupportedProtocol::UniswapV4.to_string().as_str(), filter.clone(), Some(u4))
            .auth_key(Some(config.tycho_api_key.clone()))
            .skip_state_decode_failures(true)
            .set_tokens(hmt.clone()) // ALL Tokens
            .await;

        if network.name.as_str() == "ethereum" {
            log::info!("Prebuild. Adding mainnet-specific exchanges");
            psb = psb
                .exchange::<UniswapV2State>(TychoSupportedProtocol::Sushiswap.to_string().as_str(), filter.clone(), None)
                .exchange::<UniswapV2State>(TychoSupportedProtocol::Pancakeswap.to_string().as_str(), filter.clone(), None)
                .exchange::<EVMPoolState<PreCachedDB>>(TychoSupportedProtocol::BalancerV2.to_string().as_str(), filter.clone(), Some(balancer))
                .exchange::<EVMPoolState<PreCachedDB>>(TychoSupportedProtocol::Curve.to_string().as_str(), filter.clone(), Some(curve));
        }
        OrderbookBuilder { network, psb, tokens: srzt }
    }

    pub async fn custom(network: Network, psb: ProtocolStreamBuilder, tokens: Vec<SrzToken>) -> Self {
        OrderbookBuilder { network, psb, tokens }
    }

    pub async fn build(self, config: OBPConfig, state: SharedTychoStreamState) -> Result<OrderbookProvider, StreamError> {
        log::info!("Building OBP ... (it might take a while, also depends on the API key)");
        OrderbookProvider::build(self, config, state).await
    }
}

impl OrderbookProvider {
    /// Creates a new OBP instance using a ProtocolStreamBuilder (from Tycho) with custom configuration
    /// # Arguments
    /// * `psb` - A ProtocolStreamBuilder used to build the underlying stream.
    /// * `config` - An OBPConfig allowing customization of parameters (e.g. channel capacity).
    /// * `state` - A shared state structure that is both updated internally and exposed to the client.
    /// # Returns
    /// * A Result containing the OBP instance or a StreamError if the stream could not be built.
    pub async fn build(ob: OrderbookBuilder, config: OBPConfig, state: SharedTychoStreamState) -> Result<Self, StreamError> {
        // Build the protocol stream that yields Result<BlockUpdate, StreamDecodeError>.
        match ob.psb.build().await {
            Ok(stream) => {
                let (tx, rx) = mpsc::channel(config.capacity);
                let returned = state.clone();
                let taskstate = state.clone();
                // Spawn an asynchronous task that processes the protocol stream.
                // For each message received, update the shared state and send an OBPEvent.
                log::info!("Starting stream processing task.");

                let handle = tokio::spawn(async move {
                    futures::pin_mut!(stream);
                    while let Some(update) = stream.next().await {
                        // The stream created emits BlockUpdate messages which consist of:
                        // - block number- the block this update message refers to
                        // - new_pairs- new components witnessed (either recently created or newly meeting filter criteria)
                        // - removed_pairs- components no longer tracked (either deleted due to a reorg or no longer meeting filter criteria)
                        // - states- the updated ProtocolSimstates for all components modified in this block
                        // The first message received will contain states for all protocol components registered to. Thereafter, further block updates will only contain data for updated or new components.
                        let mtx = taskstate.read().await;
                        let initialised = mtx.initialised;
                        drop(mtx);
                        match update {
                            Ok(msg) => {
                                log::info!("🔸 OBP: TychoStream: b#{} with {} states, pairs: +{} -{}", msg.block_number, msg.states.len(), msg.new_pairs.len(), msg.removed_pairs.len());
                                if !initialised {
                                    log::info!("First stream (initialised was false). Writing the entire streamed data into the shared struct.");
                                    let mut targets = vec![];
                                    for (_, comp) in msg.new_pairs.iter() {
                                        targets.push(comp.id.to_string().to_lowercase());
                                    }
                                    let mut mtx = taskstate.write().await;
                                    mtx.protosims = msg.states.clone();
                                    mtx.components = msg.new_pairs.clone();
                                    mtx.initialised = true;
                                    drop(mtx);
                                    let event = OBPEvent::Initialised(msg.block_number);
                                    let _ = tx.send(event).await;
                                } else {
                                    let mut updated = vec![];
                                    if !msg.states.is_empty() {
                                        let mut mtx = state.write().await;
                                        // log::info!("Received {} new states, updating protosims.", msg.states.len());
                                        for x in msg.states.iter() {
                                            mtx.protosims.insert(x.0.clone().to_lowercase(), x.1.clone());
                                            updated.push(x.0.clone().to_lowercase());
                                        }
                                        drop(mtx);
                                    }
                                    if !msg.new_pairs.is_empty() || !msg.removed_pairs.is_empty() {
                                        let mut mtx = state.write().await;
                                        for x in msg.new_pairs.iter() {
                                            mtx.components.insert(x.0.clone(), x.1.clone());
                                        }
                                        for x in msg.removed_pairs.iter() {
                                            mtx.components.remove(x.0);
                                        }
                                        log::info!("Received {} new pairs, and {} pairs to be removed. Updating Redis ...", msg.new_pairs.len(), msg.removed_pairs.len());
                                        drop(mtx);
                                    }
                                    let event = OBPEvent::NewHeader(msg.block_number, updated.clone());
                                    let _ = tx.send(event).await;
                                }
                            }
                            Err(err) => {
                                let event = OBPEvent::Error(err);
                                let _ = tx.send(event).await;
                            }
                        }
                    }
                });

                let obp = OrderbookProvider {
                    stream: Mutex::new(rx),
                    state: returned,
                    _handle: handle,
                    tokens: ob.tokens.clone(),
                    network: ob.network.clone(),
                };

                Ok(obp)
            }
            Err(err) => {
                log::error!("Failed to build OBP: {:?}", err.to_string());
                Err(err)
            }
        }
    }

    /// Returns components that contains the given tokens
    /// Example: target is ETH, USDC. It will return all components that contain ETH and USDC
    pub async fn get_components_for_target(&self, targets: Vec<SrzToken>) -> Vec<SrzProtocolComponent> {
        let mut output = vec![];
        let mtx = self.state.read().await;
        let comp = mtx.components.clone();
        if comp.is_empty() {
            log::error!(" 🔺 No components found in the shared state");
        }
        for (_k, v) in comp.iter() {
            let tokens: Vec<SrzToken> = v.tokens.clone().iter().map(|x| SrzToken::from(x.clone())).collect();
            if shd::core::orderbook::matchcp(tokens, targets.clone()) {
                output.push(SrzProtocolComponent::from(v.clone()));
            }
        }
        drop(mtx);
        output
    }

    pub async fn get_orderbook(&self, params: OrderbookRequestParams, simufns: Option<OrderbookFunctions>) -> Result<Orderbook, anyhow::Error> {
        let single = params.sps.is_some();
        let mtx = self.state.read().await;
        let comp = mtx.components.clone();
        let acps = comp.iter().map(|x| SrzProtocolComponent::from(x.1.clone())).collect::<Vec<SrzProtocolComponent>>(); // Not efficient at all
        let targets = params.tag.clone().split("-").map(|x| x.to_string().to_lowercase()).collect::<Vec<String>>();
        if targets.len() != 2 {
            return Err(anyhow::anyhow!("Invalid pair"));
        }
        let atks = self.tokens.clone();
        let srzt0 = atks.iter().find(|x| x.address.to_lowercase() == targets[0].clone()).ok_or_else(|| anyhow::anyhow!("Token {} not found", targets[0])).unwrap();
        let srzt1 = atks.iter().find(|x| x.address.to_lowercase() == targets[1].clone()).ok_or_else(|| anyhow::anyhow!("Token {} not found", targets[0])).unwrap();
        let targets = vec![srzt0.clone(), srzt1.clone()];
        log::info!("Building orderbook for pair {}-{} | Single point: {}", targets[0].symbol.clone(), targets[1].symbol.clone(), single);
        let (t0_to_eth_path, t0_to_eth_comps) = shd::maths::path::routing(acps.clone(), srzt0.address.to_string().to_lowercase(), self.network.eth.to_lowercase()).unwrap_or_default();
        let (t1_to_eth_path, t1_to_eth_comps) = shd::maths::path::routing(acps.clone(), srzt1.address.to_string().to_lowercase(), self.network.eth.to_lowercase()).unwrap_or_default();

        let mut to_eth_ptss: Vec<ProtoTychoState> = vec![];
        let mut ptss: Vec<ProtoTychoState> = vec![];
        for cp in acps.clone() {
            if t0_to_eth_comps.contains(&cp.id.to_lowercase()) || t1_to_eth_comps.contains(&cp.id.to_lowercase()) {
                if let Some(protosim) = mtx.protosims.get(&cp.id.to_lowercase()) {
                    to_eth_ptss.push(ProtoTychoState {
                        component: cp.clone(),
                        protosim: protosim.clone(),
                    });
                }
            }
            if shd::core::orderbook::matchcp(cp.tokens.clone(), targets.clone()) {
                if let Some(protosim) = mtx.protosims.get(&cp.id.to_lowercase()) {
                    ptss.push(ProtoTychoState {
                        component: cp.clone(),
                        protosim: protosim.clone(),
                    });
                }
            }
        }
        drop(mtx);
        if ptss.is_empty() {
            return Err(anyhow::anyhow!("No components found for the given pair"));
        }
        log::info!("Found {} components for the pair. Evaluation t0/t1 ETH value ...", ptss.len());
        let utk0_ethworth = shd::maths::path::quote(to_eth_ptss.clone(), atks.clone(), t0_to_eth_path.clone());
        let utk1_ethworth = shd::maths::path::quote(to_eth_ptss.clone(), atks.clone(), t1_to_eth_path.clone());
        match (utk0_ethworth, utk1_ethworth) {
            (Some(utk0_ethworth), Some(utk1_ethworth)) => {
                let book = shd::core::orderbook::build(self.network.clone(), ptss.clone(), targets.clone(), params.clone(), simufns, utk0_ethworth, utk1_ethworth).await;
                Ok(book)
            }
            _ => Err(anyhow::anyhow!("Failed to quote the pair in ETH")),
        }
    }

    /// Generates the struct param to build an orderbook
    /// Min_comps is the minimum number of components that the pair should have (= liquidity pools), the higher it is, the more iterations it will take to find a pair
    pub async fn generate_random_orderbook_params(&self, min_comps: usize) -> OrderbookRequestParams {
        log::info!("Generating random orderbook ...");
        let seed = [42u8; 32]; // 256-bit seed
        let mut rng = StdRng::from_seed(seed);
        let tokens = self.tokens.clone();
        let size = tokens.len();
        let mut iterations = 0;
        let mut components = vec![];
        let mut tag = "".to_string();
        while components.len() < min_comps {
            let t0 = rng.gen_range(1..=size - 1);
            let token0 = tokens.get(t0).unwrap();
            let token1 = tokens.get(t0 - 1).unwrap();
            let tgcps = self.get_components_for_target(vec![token0.clone(), token1.clone()]).await;
            if tgcps.len() >= min_comps {
                if token0.symbol == *"WETH" || token1.symbol == *"WETH" || token0.symbol == *"SolvBTC" || token1.symbol == *"SolvBTC" {
                    continue;
                }
                log::info!(
                    "Got {} components found for pair >>> {}  🔄  {} ({}-{}) (after {} iterations)",
                    tgcps.len(),
                    token0.symbol.clone(),
                    token1.symbol.clone(),
                    token0.address.clone(),
                    token1.address.clone(),
                    iterations
                );

                tag = format!("{}-{}", token0.address.to_lowercase(), token1.address.to_lowercase());
                components = tgcps;
            } else {
                if iterations % 1000 == 0 {
                    log::info!("No components found for pair {}-{} (iterations # {})", token0.symbol.clone(), token1.symbol.clone(), iterations);
                }
                iterations += 1;
            }
        }
        OrderbookRequestParams { tag, sps: None }
    }
    pub async fn depth(&self) {} // with Option

    // ToDo: traits/interfaces
}
