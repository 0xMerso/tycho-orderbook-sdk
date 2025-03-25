use alloy::rpc::types::Block;
use futures::{Stream, StreamExt};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tycho_simulation::evm::decoder::StreamDecodeError;
use tycho_simulation::protocol::models::BlockUpdate;

use std::str::FromStr;

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

// Assume these imports point to your existing types.
use tycho_client::stream::StreamError;

use super::types::SharedTychoStreamState; // adjust module path as needed

/// Enum representing the events that can be sent to the client.
#[derive(Debug)]
pub enum OBPEvent {
    /// A successful block update received from the ProtocolStreamBuilder.
    // BlockUpdate(BlockUpdate),
    BlockNumber(u64),
    /// An error encountered during stream decoding.
    Error(StreamDecodeError),
}

/// Configuration for the OBP provider.
#[derive(Clone)]
pub struct OBPConfig {
    /// The capacity of the message channel.
    pub capacity: usize,
}

impl Default for OBPConfig {
    fn default() -> Self {
        OBPConfig { capacity: 100 }
    }
}

/// -- Tycho -- Orderbook Provider (OBP) that wraps a ProtocolStreamBuilder stream.
/// It forwards block updates as events to the client while sharing an internal state.
pub struct OBP {
    /// Receiver side of the channel where OBPEvents are sent.
    pub stream: mpsc::Receiver<OBPEvent>,
    /// The shared state, accessible both to the internal task and the client.
    pub state: SharedTychoStreamState,
    /// The spawned task handle is stored to ensure the task remains running.
    _handle: JoinHandle<()>,
}

impl OBP {
    /// Creates a new OBP instance using a ProtocolStreamBuilder with custom configuration.
    /// # Arguments
    /// * `psb` - A ProtocolStreamBuilder used to build the underlying stream.
    /// * `config` - An OBPConfig allowing customization of parameters (e.g. channel capacity).
    /// * `state` - A shared state structure that is both updated internally and exposed to the client.
    pub async fn new(psb: ProtocolStreamBuilder, config: OBPConfig, state: SharedTychoStreamState) -> Result<Self, StreamError> {
        // Build the protocol stream that yields Result<BlockUpdate, StreamDecodeError>.
        let protocol_stream = psb.build().await.unwrap();
        let (tx, rx) = mpsc::channel(config.capacity);
        let dupstate = state.clone();
        // Spawn an asynchronous task that processes the protocol stream.
        // For each message received, update the shared state and send an OBPEvent.
        let handle = tokio::spawn(async move {
            futures::pin_mut!(protocol_stream);
            while let Some(item) = protocol_stream.next().await {
                match item {
                    Ok(update) => {
                        // Update the shared state based on the block update.
                        let mut mtx = dupstate.write().await;
                        drop(mtx);
                        // TODO: Update state.protosims or state.components based on `update`.
                        // state.protosims.insert("some_key".to_string(), some_value);
                        // let event = OBPEvent::BlockUpdate(update);
                        let event = OBPEvent::BlockNumber(update.block_number);
                        let _ = tx.send(event).await;
                    }
                    Err(err) => {
                        let event = OBPEvent::Error(err);
                        let _ = tx.send(event).await;
                    }
                }
            }
        });

        Ok(OBP { stream: rx, state, _handle: handle })
    }
}
