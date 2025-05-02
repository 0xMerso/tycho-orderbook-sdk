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
use tycho_simulation::protocol::models::ProtocolComponent;

use crate::builder::OrderbookBuilderConfig;
use crate::data::fmt::SrzProtocolComponent;
use crate::types;
use crate::types::Network;

use crate::types::TychoSupportedProtocol;

/// Get the default protocol stream builder
/// But any other configuration of ProtocolStreamBuilder can be used to build an orderbook
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

    tracing::debug!("Tycho endpoint: {} and chain: {}", network.tycho, chain);
    let mut psb = ProtocolStreamBuilder::new(&network.tycho, chain)
        .exchange::<UniswapV2State>(TychoSupportedProtocol::UniswapV2.to_string().as_str(), filter.clone(), None)
        .exchange::<UniswapV3State>(TychoSupportedProtocol::UniswapV3.to_string().as_str(), filter.clone(), None)
        .exchange::<UniswapV4State>(TychoSupportedProtocol::UniswapV4.to_string().as_str(), filter.clone(), Some(u4))
        .auth_key(Some(apikey.clone()))
        .skip_state_decode_failures(true)
        .set_tokens(hmt.clone()) // ALL Tokens
        .await;

    if network.name.as_str() == "ethereum" {
        tracing::trace!("Adding mainnet-specific exchanges");
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

/// Get the original components from the list of components
/// Used when Tycho packages require the exact components
/// Conversion from:: SrzProtocolComponent to ProtocolComponent doesn't work. Idk why.
pub fn get_original_components(originals: HashMap<String, ProtocolComponent>, targets: Vec<SrzProtocolComponent>) -> Vec<ProtocolComponent> {
    let mut filtered = Vec::with_capacity(targets.len());
    for cp in targets.clone().iter().enumerate() {
        let tgt = cp.1.id.to_string().to_lowercase();
        if let Some(original) = originals.get(&tgt) {
            filtered.push(original.clone());
        } else {
            tracing::warn!("OBP Event: Error: Component {} not found in the original list, anormal !", tgt);
        }
    }
    if filtered.len() != targets.len() {
        tracing::error!("Execution error: not all components found in the original list, anormal !");
    }
    let order: HashMap<String, usize> = targets.iter().enumerate().map(|(i, item)| (item.id.to_string().to_lowercase(), i)).collect();
    filtered.sort_by_key(|item| order.get(&item.id.to_string().to_lowercase()).copied().unwrap_or(usize::MAX));
    // --- Logs ---
    // for o in filtered.iter() {
    //     tracing::trace!(" - originals : {}", o.id);
    //     let attributes = o.static_attributes.clone();
    //     for a in attributes.iter() {
    //         tracing::trace!("   - {}: {}", a.0, a.1);
    //     }
    // }
    // for t in targets.iter() {
    //     tracing::trace!(" - targets   : {}", t.id);
    //     let attributes = t.static_attributes.clone();
    //     for a in attributes.iter() {
    //         tracing::trace!("   - {}: {}", a.0, a.1);
    //     }
    // }
    filtered
}
