use tycho_simulation::models::Token;
use tycho_simulation::tycho_client::stream::StreamError;


use tycho_simulation::evm::stream::ProtocolStreamBuilder;

use crate::data::fmt::SrzToken;
use crate::provider::OrderbookProvider;
use crate::types::Network;
use crate::types::OrderbookBuilder;
use crate::types::SharedTychoStreamState;

/// OrderbookBuilder is a struct that allows the creation of an OrderbookProvider instance, using a default or custom ProtocolStreamBuilder from Tycho.
impl OrderbookBuilder {
    /// Default logic to create a ProtocolStreamBuilder, used to build a OrderbookProvider
    /// For more advanced use-cases, you can create your own ProtocolStreamBuilder and pass it to custom() fn
    pub fn new(network: Network, psb: ProtocolStreamBuilder, apikey: String, tokens: Vec<Token>) -> Self {
        let mut srztokens = vec![];
        tokens.iter().for_each(|t| {
            srztokens.push(SrzToken::from(t.clone()));
        });
        OrderbookBuilder {
            network,
            psb,
            tokens: srztokens,
            apikey: Some(apikey.clone()),
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

    pub fn apikey(mut self, apikey: Option<String>) -> Self {
        self.apikey = apikey;
        self
    }

    // Default ProtocolStreamBuilder
    pub async fn build(self, state: SharedTychoStreamState) -> Result<OrderbookProvider, StreamError> {
        tracing::debug!("Building OrderbookProvider ... (it might take a while depending the API key)");
        OrderbookProvider::new(self, state).await
    }
}
