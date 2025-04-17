use tycho_simulation::evm::stream::ProtocolStreamBuilder;
use tycho_simulation::models::Token;
use tycho_simulation::tycho_client::stream::StreamError;

use crate::core::helper::default_protocol_stream_builder;
use crate::core::solver::DefaultOrderbookSolver;
use crate::data::fmt::SrzToken;
use crate::provider::OrderbookProvider;
use crate::types::Network;
use crate::utils::r#static::filter::ADD_TVL_THRESHOLD;
use tycho_simulation::tycho_client::feed::component_tracker::ComponentFilter;

#[derive(Clone)]
pub struct OrderbookBuilderConfig {
    pub filter: ComponentFilter,
}

pub struct OrderbookBuilder {
    pub network: Network,
    pub psb: ProtocolStreamBuilder,
    pub tokens: Vec<SrzToken>,
    pub key: Option<String>,
}

/// OrderbookBuilder is a struct that allows the creation of an OrderbookProvider instance, using a default or custom ProtocolStreamBuilder from Tycho.
impl OrderbookBuilder {
    /// Default logic to create a ProtocolStreamBuilder, used to build a OrderbookProvider
    /// For more advanced use-cases, you can create your own ProtocolStreamBuilder and pass it to custom() fn
    pub async fn new(network: Network, psb: Option<ProtocolStreamBuilder>, key: String, tokens: Vec<Token>) -> Self {
        let psb = match psb {
            Some(psb) => psb,
            None => {
                // --- Create Protocol stream builder --- Create your own protocol stream builder if you want to custom it.
                let filter = ComponentFilter::with_tvl_range(ADD_TVL_THRESHOLD, ADD_TVL_THRESHOLD);
                default_protocol_stream_builder(network.clone(), key.clone(), OrderbookBuilderConfig { filter }, tokens.clone()).await
            }
        };
        let mut srztokens = vec![];
        tokens.iter().for_each(|t| {
            srztokens.push(SrzToken::from(t.clone()));
        });
        OrderbookBuilder {
            network,
            psb,
            tokens: srztokens,
            key: Some(key.clone()),
        }
    }

    pub fn network(mut self, network: Network) -> Self {
        self.network = network;
        self
    }

    pub fn psb(mut self, psb: ProtocolStreamBuilder) -> Self {
        self.psb = psb;
        self
    }

    pub fn tokens(mut self, tokens: Vec<SrzToken>) -> Self {
        self.tokens = tokens;
        self
    }

    pub fn key(mut self, key: Option<String>) -> Self {
        self.key = key;
        self
    }

    // Default ProtocolStreamBuilder
    pub async fn build(self) -> Result<OrderbookProvider<DefaultOrderbookSolver>, StreamError> {
        tracing::debug!("Building OrderbookProvider ... (with env API key)");
        OrderbookProvider::new(self.network, self.psb, self.tokens, self.key.clone(), DefaultOrderbookSolver).await
    }
}
