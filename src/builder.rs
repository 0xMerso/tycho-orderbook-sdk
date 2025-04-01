use std::collections::HashMap;
use tycho_simulation::models::Token;
use tycho_simulation::tycho_client::stream::StreamError;

use tycho_simulation::evm::protocol::filters::curve_pool_filter;
use tycho_simulation::evm::protocol::filters::uniswap_v4_pool_with_hook_filter;
use tycho_simulation::evm::protocol::uniswap_v3::state::UniswapV3State;
use tycho_simulation::evm::protocol::uniswap_v4::state::UniswapV4State;

use tycho_simulation::evm::{
    engine_db::tycho_db::PreCachedDB,
    protocol::{filters::balancer_pool_filter, uniswap_v2::state::UniswapV2State, vm::state::EVMPoolState},
    stream::ProtocolStreamBuilder,
};

use crate::core::rpc;
use crate::data::fmt::SrzToken;
use crate::types;
use crate::types::EnvConfig;
use crate::types::Network;
use crate::types::OrderbookBuilder;
use crate::types::OrderbookBuilderConfig;
use crate::types::OrderbookProvider;
use crate::types::OrderbookProviderConfig;
use crate::types::SharedTychoStreamState;
use crate::types::TychoSupportedProtocol;

/// OrderbookBuilder is a struct that allows the creation of an OrderbookProvider instance, using a default or custom ProtocolStreamBuilder from Tycho.
impl OrderbookBuilder {
    /**
     * Default logic to create a ProtocolStreamBuilder, used to build a OrderbookProvider
     * For more advanced use-cases, you can create your own ProtocolStreamBuilder and pass it to custom() fn
     */
    pub async fn new(network: Network, env: EnvConfig, config: OrderbookBuilderConfig, tokens: Option<Vec<Token>>) -> Self {
        let (_, _, chain) = types::chain(network.name.clone()).expect("Invalid chain");
        let u4 = uniswap_v4_pool_with_hook_filter;
        let balancer = balancer_pool_filter;
        let curve = curve_pool_filter;
        let filter = config.filter.clone();
        let tokens = match tokens {
            Some(t) => t,
            None => rpc::tokens(&network, &env).await.unwrap(),
        };
        let mut hmt = HashMap::new();
        let mut srzt = vec![];
        tokens.iter().for_each(|t| {
            hmt.insert(t.address.clone(), t.clone());
            srzt.push(SrzToken::from(t.clone()));
        });
        let mut psb = ProtocolStreamBuilder::new(&network.tycho, chain)
            .exchange::<UniswapV2State>(TychoSupportedProtocol::UniswapV2.to_string().as_str(), filter.clone(), None)
            .exchange::<UniswapV3State>(TychoSupportedProtocol::UniswapV3.to_string().as_str(), filter.clone(), None)
            .exchange::<UniswapV4State>(TychoSupportedProtocol::UniswapV4.to_string().as_str(), filter.clone(), Some(u4))
            .auth_key(Some(env.tycho_api_key.clone()))
            .skip_state_decode_failures(true)
            .set_tokens(hmt.clone()) // ALL Tokens
            .await;

        if network.name.as_str() == "ethereum" {
            tracing::trace!("Prebuild. Adding mainnet-specific exchanges");
            psb = psb
                .exchange::<UniswapV2State>(TychoSupportedProtocol::Sushiswap.to_string().as_str(), filter.clone(), None)
                .exchange::<UniswapV2State>(TychoSupportedProtocol::Pancakeswap.to_string().as_str(), filter.clone(), None)
                .exchange::<EVMPoolState<PreCachedDB>>(TychoSupportedProtocol::BalancerV2.to_string().as_str(), filter.clone(), Some(balancer))
                .exchange::<EVMPoolState<PreCachedDB>>(TychoSupportedProtocol::Curve.to_string().as_str(), filter.clone(), Some(curve));
        }
        OrderbookBuilder {
            network,
            psb,
            tokens: srzt,
            apikey: Some(env.tycho_api_key.clone()),
        }
    }

    pub async fn custom(network: Network, psb: ProtocolStreamBuilder, tokens: Vec<SrzToken>, apikey: String) -> Self {
        OrderbookBuilder {
            network,
            psb,
            tokens,
            apikey: Some(apikey),
        }
    }

    pub async fn build(self, config: OrderbookProviderConfig, state: SharedTychoStreamState) -> Result<OrderbookProvider, StreamError> {
        tracing::debug!("Building OBP ... (it might take a while depending the API key)");
        OrderbookProvider::build(self, config, state).await
    }
}
