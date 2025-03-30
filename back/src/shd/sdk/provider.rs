use futures::StreamExt;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::shd;
use crate::shd::types::{OBPEvent, OrderbookProvider};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tycho_simulation::tycho_client::stream::StreamError;

use super::super::data::fmt::SrzProtocolComponent;
use super::super::data::fmt::SrzToken;
use super::super::types::SharedTychoStreamState;
use super::super::types::{OBPConfig, OrderbookRequestParams};
use super::super::types::{Orderbook, OrderbookBuilder};
use super::super::types::{OrderbookFunctions, ProtoTychoState};

/// OrderbookProvider is a struct that manages the protocol stream and shared state, and provides methods to interact with the stream, build orderbooks, and more.
///
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
                                log::info!(
                                    "🔸 OBP: TychoStream: b#{} with {} states, pairs: +{} -{}",
                                    msg.block_number,
                                    msg.states.len(),
                                    msg.new_pairs.len(),
                                    msg.removed_pairs.len()
                                );
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
                    apikey: ob.api_token.clone(),
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
            if shd::core::book::matchcp(tokens, targets.clone()) {
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
        let srzt0 = atks
            .iter()
            .find(|x| x.address.to_lowercase() == targets[0].clone())
            .ok_or_else(|| anyhow::anyhow!("Token {} not found", targets[0]))
            .unwrap();
        let srzt1 = atks
            .iter()
            .find(|x| x.address.to_lowercase() == targets[1].clone())
            .ok_or_else(|| anyhow::anyhow!("Token {} not found", targets[0]))
            .unwrap();
        let targets = vec![srzt0.clone(), srzt1.clone()];
        log::info!("Building orderbook for pair {}-{} | Single point: {}", targets[0].symbol.clone(), targets[1].symbol.clone(), single);
        let (base_to_eth_path, base_to_eth_comps) = shd::maths::path::routing(acps.clone(), srzt0.address.to_string().to_lowercase(), self.network.eth.to_lowercase()).unwrap_or_default();
        let (quote_to_eth_path, quote_to_eth_comps) = shd::maths::path::routing(acps.clone(), srzt1.address.to_string().to_lowercase(), self.network.eth.to_lowercase()).unwrap_or_default();

        let mut to_eth_ptss: Vec<ProtoTychoState> = vec![];
        let mut ptss: Vec<ProtoTychoState> = vec![];
        for cp in acps.clone() {
            if base_to_eth_comps.contains(&cp.id.to_lowercase()) || quote_to_eth_comps.contains(&cp.id.to_lowercase()) {
                if let Some(protosim) = mtx.protosims.get(&cp.id.to_lowercase()) {
                    to_eth_ptss.push(ProtoTychoState {
                        component: cp.clone(),
                        protosim: protosim.clone(),
                    });
                }
            }
            if shd::core::book::matchcp(cp.tokens.clone(), targets.clone()) {
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
        let unit_base_eth_worth = shd::maths::path::quote(to_eth_ptss.clone(), atks.clone(), base_to_eth_path.clone());
        let unit_quote_eth_worth = shd::maths::path::quote(to_eth_ptss.clone(), atks.clone(), quote_to_eth_path.clone());
        match (unit_base_eth_worth, unit_quote_eth_worth) {
            (Some(unit_base_eth_worth), Some(unit_quote_eth_worth)) => Ok(shd::core::book::build(
                self.network.clone(),
                self.apikey.clone(),
                ptss.clone(),
                targets.clone(),
                params.clone(),
                simufns,
                unit_base_eth_worth,
                unit_quote_eth_worth,
            )
            .await),
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
}
