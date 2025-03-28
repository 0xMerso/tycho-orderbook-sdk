use crate::shd::types::Network;

/// Implement conversion from Orderbook to a standard Orderbook format like Binance
/// Binance: https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-endpoints

/// --- Read ---
/// GET /api/v3/exchangeInfo --> not implemented (not needed)
/// GET /api/v3/depth        --> implemented
/// GET /api/v3/trades       --> not possible to implement unless using an external RPC, out of scope ?
/// GET /api/v3/avgPrice     --> implemented

/// --- Write ---
/// POST /api/v3/order
/// POST /api/v3/order/test
/// GET /api/v3/order

/// --- Helpers ---

/// Convert the base and quote to a tag format, used in Orderbook query params
pub fn symtag(network: Network, base: &str, quote: &str) -> Option<String> {
    let base = match base.to_lowercase().as_str() {
        "eth" | "weth" => network.eth.clone(),
        "usdc" => network.usdc.clone(),
        "usdt" => network.usdt.clone(),
        "btc" | "wbtc" => network.wbtc.clone(),
        "dai" => network.dai.clone(),
        _ => String::default(),
    };
    let quote = match quote.to_lowercase().as_str() {
        "eth" | "weth" => network.eth.clone(),
        "usdc" => network.usdc.clone(),
        "usdt" => network.usdt.clone(),
        "btc" | "wbtc" => network.wbtc.clone(),
        "dai" => network.dai.clone(),
        _ => String::default(),
    };
    let tag = format!("{}-{}", base.to_lowercase(), quote.to_lowercase());
    if tag.is_empty() {
        log::error!("Failed to convert base and quote to tag");
        return None;
    }
    Some(tag)
}
