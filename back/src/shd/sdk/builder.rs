use futures::StreamExt;

use std::collections::HashMap;
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
use crate::shd::types::{OrderbookProvider, TychoSupportedProtocol};

use super::super::data::fmt::SrzToken;
use super::super::types::OBPConfig;
use super::super::types::OrderbookBuilder;
use super::super::types::{EnvConfig, Network, SharedTychoStreamState};

/// OrderbookBuilder is a struct that allows the creation of an OrderbookProvider instance, using a default or custom ProtocolStreamBuilder from Tycho.
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
            None => shd::core::rpc::tokens(&network, &config).await.unwrap(),
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
        OrderbookBuilder {
            network,
            psb,
            tokens: srzt,
            api_token: Some(config.tycho_api_key.clone()),
        }
    }

    pub async fn custom(network: Network, psb: ProtocolStreamBuilder, tokens: Vec<SrzToken>, api_token: String) -> Self {
        OrderbookBuilder {
            network,
            psb,
            tokens,
            api_token: Some(api_token),
        }
    }

    pub async fn build(self, config: OBPConfig, state: SharedTychoStreamState) -> Result<OrderbookProvider, StreamError> {
        log::info!("Building OBP ... (it might take a while, also depends on the API key)");
        OrderbookProvider::build(self, config, state).await
    }
}
