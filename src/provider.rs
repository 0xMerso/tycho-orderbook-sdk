use futures::StreamExt;

use tokio::task::JoinHandle;

use crate::builder::OrderbookBuilder;
use crate::core::book::{self};
use crate::core::solver::{DefaultOrderbookSolver, OrderbookSolver};

use crate::types::{self, Network, OrderbookEvent};
use crate::{data, maths};

use data::fmt::SrzProtocolComponent;
use data::fmt::SrzToken;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tycho_simulation::tycho_client::stream::StreamError;
use types::Orderbook;
use types::OrderbookRequestParams;
use types::ProtoSimComp;
use types::SharedTychoStreamState;

/// Orderbook Provider Configuration
#[derive(Clone)]
pub struct OrderbookProviderConfig {
    // The capacity of the channel used to send OrderbookEvents.
    pub capacity: usize,
}

impl Default for OrderbookProviderConfig {
    fn default() -> Self {
        OrderbookProviderConfig { capacity: 100 }
    }
}

/// SDK prderbook provider (OBP) that wraps a ProtocolStreamBuistrlder stream
pub struct OrderbookProvider<S: OrderbookSolver = DefaultOrderbookSolver> {
    /// The spawned task handle is stored to ensure the task remains running.
    pub _handle: JoinHandle<()>,
    /// Tokens given by Tycho
    pub tokens: Vec<SrzToken>,
    /// The network used
    pub network: Network,
    /// Receiver side of the channel where OrderbookEvents are sent.
    pub stream: Mutex<mpsc::Receiver<OrderbookEvent>>, // mpsc::Receiver<OrderbookEvent>,
    // pub stream: mpsc::Receiver<OrderbookEvent>, // mpsc::Receiver<OrderbookEvent>,
    /// The shared state, accessible both to the internal task and the client.
    pub state: SharedTychoStreamState,
    /// The API token used to facilitate the Tycho queries
    pub apikey: Option<String>,
    /// The solver instance used to optimize trades.
    pub solver: S,
}

/// OrderbookProvider is a struct that manages the protocol stream and shared state, and provides methods to interact with the stream, build orderbooks, and more.
impl OrderbookProvider {
    /// Creates a new OBP instance using a ProtocolStreamBuilder (from Tycho) with custom configuration
    /// # Arguments
    /// * `psb` - A ProtocolStreamBuilder used to build the underlying stream.
    /// * `config` - An OrderbookProviderConfig allowing customization of parameters (e.g. channel capacity).
    /// * `state` - A shared state structure that is both updated internally and exposed to the client.
    /// # Returns
    /// * A Result containing the OBP instance or a StreamError if the stream could not be built.
    pub async fn new<S>(ob: OrderbookBuilder, state: SharedTychoStreamState, solver: S) -> Result<OrderbookProvider<S>, StreamError>
    where
        S: OrderbookSolver + 'static,
    {
        // Build the protocol stream that yields Result<BlockUpdate, StreamDecodeError>.
        match ob.psb.build().await {
            Ok(stream) => {
                let (sender, receiver) = mpsc::channel(100);
                let shared = state.clone();
                // Spawn an asynchronous task that processes the protocol stream.
                // For each message received, update the shared state and send an OrderbookEvent.
                tracing::debug!("Starting stream processing task.");
                let handle = tokio::spawn(async move {
                    futures::pin_mut!(stream);
                    while let Some(update) = stream.next().await {
                        // The stream created emits BlockUpdate messages which consist of:
                        // - block number- the block this update message refers to
                        // - new_pairs- new components witnessed (either recently created or newly meeting filter criteria)
                        // - removed_pairs- components no longer tracked (either deleted due to a reorg or no longer meeting filter criteria)
                        // - states- the updated ProtocolSimstates for all components modified in this block
                        // The first message received will contain states for all protocol components registered to. Thereafter, further block updates will only contain data for updated or new components.
                        let mtx = state.read().await;
                        let initialised = mtx.initialised;
                        drop(mtx);
                        match update {
                            Ok(msg) => {
                                tracing::debug!(
                                    "🔸 OBP: TychoStream: b#{} with {} states, pairs: +{} -{}",
                                    msg.block_number,
                                    msg.states.len(),
                                    msg.new_pairs.len(),
                                    msg.removed_pairs.len()
                                );
                                if !initialised {
                                    tracing::debug!("First stream (initialised was false). Writing the entire streamed data into the shared struct.");
                                    let mut targets = vec![];
                                    for (_, comp) in msg.new_pairs.iter() {
                                        // tracing::debug!("Adding new component {} to the shared state: {}", comp.protocol_system.clone(), comp.protocol_type_name.clone());
                                        targets.push(comp.id.to_string().to_lowercase());
                                    }
                                    let mut writing = state.write().await;
                                    writing.protosims = msg.states.clone();
                                    writing.components = msg.new_pairs.clone();
                                    writing.initialised = true;
                                    drop(writing);
                                    let event = OrderbookEvent::Initialised(msg.block_number);
                                    let _ = sender.send(event).await;
                                } else {
                                    let mut updated = vec![];
                                    if !msg.states.is_empty() {
                                        let mut writing = state.write().await;

                                        for x in msg.states.iter() {
                                            writing.protosims.insert(x.0.clone().to_lowercase(), x.1.clone());
                                            updated.push(x.0.clone().to_lowercase());
                                        }
                                        drop(writing);
                                    }
                                    if !msg.new_pairs.is_empty() || !msg.removed_pairs.is_empty() {
                                        let mut writing = state.write().await;
                                        for x in msg.new_pairs.iter() {
                                            writing.components.insert(x.0.clone().to_lowercase(), x.1.clone());
                                        }
                                        for x in msg.removed_pairs.iter() {
                                            writing.components.remove(&x.0.clone().to_lowercase());
                                        }
                                        tracing::debug!("Received {} new pairs, and {} pairs to be removed. Updating Redis ...", msg.new_pairs.len(), msg.removed_pairs.len());
                                        drop(writing);
                                    }
                                    let event = OrderbookEvent::NewHeader(msg.block_number, updated.clone());
                                    let _ = sender.send(event).await;
                                }
                            }
                            Err(err) => {
                                let event = OrderbookEvent::Error(err);
                                let _ = sender.send(event).await;
                            }
                        }
                    }
                });

                let obp = OrderbookProvider {
                    stream: Mutex::new(receiver),
                    state: shared, // ---> Anormal here, but it works, need to clarify. Arc pointing to the same memory location, it should be ok, but incoherent to need dup
                    _handle: handle,
                    tokens: ob.tokens.clone(),
                    network: ob.network.clone(),
                    apikey: ob.apikey.clone(),
                    solver,
                };

                Ok(obp)
            }
            Err(err) => {
                tracing::error!(
                    "Failed to create stream abd build orderbook provider: {:?}. Retry by changing the Tycho Stream filters, or with a dedicated API key",
                    err.to_string()
                );

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
            tracing::error!(" 🔺 No components found in the shared state");
        }
        for (_k, v) in comp.iter() {
            let tokens: Vec<SrzToken> = v.tokens.clone().iter().map(|x| SrzToken::from(x.clone())).collect();
            if book::matchcp(tokens, targets.clone()) {
                output.push(SrzProtocolComponent::from(v.clone()));
            }
        }
        drop(mtx);
        output
    }

    /// Compute the orderbook for the given pair by simulating trades on the components matching the requested pair
    pub async fn get_orderbook<S: OrderbookSolver>(&self, solver: S, params: OrderbookRequestParams) -> Result<Orderbook, anyhow::Error> {
        let single = params.point.is_some();
        let mtx = self.state.read().await;
        let comp = mtx.components.clone();
        drop(mtx);
        let acps = comp.iter().map(|x| SrzProtocolComponent::from(x.1.clone())).collect::<Vec<SrzProtocolComponent>>(); // Not efficient at all

        // --- Check if the pair is valid ---
        let targets = params.tag.clone().split("-").map(|x| x.to_string().to_lowercase()).collect::<Vec<String>>();
        if targets.len() != 2 {
            return Err(anyhow::anyhow!("Invalid pair"));
        }
        let all_tokens = self.tokens.clone();
        let srzt0 = all_tokens
            .iter()
            .find(|x| x.address.to_lowercase() == targets[0].clone())
            .ok_or_else(|| anyhow::anyhow!("Token not found: {}", targets[0]));
        let srzt1 = all_tokens
            .iter()
            .find(|x| x.address.to_lowercase() == targets[1].clone())
            .ok_or_else(|| anyhow::anyhow!("Token not found: {}", targets[0]));
        let (srzt0, srzt1) = match (srzt0, srzt1) {
            (Ok(t0), Ok(t1)) => (t0.clone(), t1.clone()),
            (Err(e), _) => return Err(e),
            (_, Err(e)) => return Err(e),
        };

        let targets = vec![srzt0.clone(), srzt1.clone()];
        tracing::debug!("Building orderbook for pair {}-{} | Single point: {}", targets[0].symbol.clone(), targets[1].symbol.clone(), single);
        // --- Compute path ---
        let base_to_eth: Result<types::ValorisationPath, String> = maths::path::routing(acps.clone(), srzt0.address.to_string().to_lowercase(), self.network.eth.to_lowercase());
        let quote_to_eth: Result<types::ValorisationPath, String> = maths::path::routing(acps.clone(), srzt1.address.to_string().to_lowercase(), self.network.eth.to_lowercase());
        match (base_to_eth, quote_to_eth) {
            (Ok(base_to_eth), Ok(quote_to_eth)) => {
                let mut to_eth_pts: Vec<ProtoSimComp> = vec![];
                let mut pts: Vec<ProtoSimComp> = vec![];
                let mtx = self.state.read().await;
                for cp in acps.clone() {
                    if base_to_eth.comp_path.contains(&cp.id.to_lowercase()) || quote_to_eth.comp_path.contains(&cp.id.to_lowercase()) {
                        if let Some(protosim) = mtx.protosims.get(&cp.id.to_lowercase()) {
                            to_eth_pts.push(ProtoSimComp {
                                component: cp.clone(),
                                protosim: protosim.clone(),
                            });
                        }
                    }
                    if book::matchcp(cp.tokens.clone(), targets.clone()) {
                        if let Some(protosim) = mtx.protosims.get(&cp.id.to_lowercase()) {
                            pts.push(ProtoSimComp {
                                component: cp.clone(),
                                protosim: protosim.clone(),
                            });
                        }
                    }
                }
                drop(mtx);
                if pts.is_empty() {
                    return Err(anyhow::anyhow!("No components found for the given pair"));
                }
                tracing::debug!("Found {} components for the pair. Evaluation t0/t1 ETH value ...", pts.len());
                let unit_base_eth_worth = maths::path::quote(to_eth_pts.clone(), all_tokens.clone(), base_to_eth.token_path.clone());
                let unit_quote_eth_worth = maths::path::quote(to_eth_pts.clone(), all_tokens.clone(), quote_to_eth.token_path.clone());
                match (unit_base_eth_worth, unit_quote_eth_worth) {
                    (Some(unit_base_eth_worth), Some(unit_quote_eth_worth)) => {
                        book::build(
                            solver,
                            self.network.clone(),
                            self.apikey.clone(),
                            pts.clone(),
                            targets.clone(),
                            params.clone(),
                            unit_base_eth_worth,
                            unit_quote_eth_worth,
                        )
                        .await
                    }
                    _ => Err(anyhow::anyhow!("Failed to quote the pair in ETH")),
                }
            }
            (Err(e), _) => Err(anyhow::anyhow!(e)),
            (_, Err(e)) => Err(anyhow::anyhow!(e)),
        }
    }
}
