use std::collections::HashMap;
use tycho_simulation::evm::protocol::ekubo::state::EkuboState;
use tycho_simulation::evm::protocol::filters::{balancer_pool_filter, curve_pool_filter, uniswap_v4_pool_with_hook_filter};
use tycho_simulation::models::Token;

use tycho_simulation::evm::protocol::uniswap_v3::state::UniswapV3State;
use tycho_simulation::evm::protocol::uniswap_v4::state::UniswapV4State;

use tycho_simulation::evm::{
    engine_db::tycho_db::PreCachedDB,
    protocol::{uniswap_v2::state::UniswapV2State, vm::state::EVMPoolState},
    stream::ProtocolStreamBuilder,
};

use crate::types;
use crate::types::Network;
use crate::types::OrderbookBuilderConfig;
use crate::types::TychoSupportedProtocol;

pub async fn default_protocol_stream_builder(network: Network, apikey: String, config: OrderbookBuilderConfig, tokens: Vec<Token>) -> ProtocolStreamBuilder {
    let (_, _, chain) = types::chain(network.name.clone()).expect("Invalid chain");
    let u4 = uniswap_v4_pool_with_hook_filter;
    let balancer = balancer_pool_filter;
    let curve = curve_pool_filter;
    let filter = config.filter.clone();

    let mut hmt = HashMap::new();
    tokens.iter().for_each(|t| {
        hmt.insert(t.address.clone(), t.clone());
    });

    tracing::debug!("network.tycho: {} and chain: {}", network.tycho, chain);
    let mut psb = ProtocolStreamBuilder::new(&network.tycho, chain)
        .exchange::<UniswapV2State>(TychoSupportedProtocol::UniswapV2.to_string().as_str(), filter.clone(), None)
        .exchange::<UniswapV3State>(TychoSupportedProtocol::UniswapV3.to_string().as_str(), filter.clone(), None)
        .exchange::<UniswapV4State>(TychoSupportedProtocol::UniswapV4.to_string().as_str(), filter.clone(), Some(u4))
        .auth_key(Some(apikey.clone()))
        .skip_state_decode_failures(true)
        .set_tokens(hmt.clone()) // ALL Tokens
        .await;

    if network.name.as_str() == "ethereum" {
        tracing::trace!("Prebuild. Adding mainnet-specific exchanges");
        psb = psb
            .exchange::<UniswapV2State>(TychoSupportedProtocol::Sushiswap.to_string().as_str(), filter.clone(), None)
            .exchange::<UniswapV2State>(TychoSupportedProtocol::PancakeswapV2.to_string().as_str(), filter.clone(), None)
            .exchange::<UniswapV3State>(TychoSupportedProtocol::PancakeswapV3.to_string().as_str(), filter.clone(), None)
            .exchange::<EkuboState>(TychoSupportedProtocol::EkuboV2.to_string().as_str(), filter.clone(), None)
            .exchange::<EVMPoolState<PreCachedDB>>(TychoSupportedProtocol::BalancerV2.to_string().as_str(), filter.clone(), Some(balancer))
            .exchange::<EVMPoolState<PreCachedDB>>(TychoSupportedProtocol::Curve.to_string().as_str(), filter.clone(), Some(curve));
    }
    psb
}
