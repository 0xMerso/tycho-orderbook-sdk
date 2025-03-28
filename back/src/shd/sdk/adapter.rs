use std::cmp::min;

use crate::shd::types::{Network, Orderbook, OrderbookDepth};

/// Implement conversion from Orderbook to a standard Orderbook format like Binance
/// Binance: https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-endpoints

/// ======================================================= Read =======================================================
/// GET /api/v3/exchangeInfo --> not implemented (not needed)
/// GET /api/v3/depth        --> implemented

impl Orderbook {
    /// Get the orderbook depth
    /// Because onchain liquidity is not splitted by orders like in a traditional orderbook, this function is approximate
    /// It depends on the amounts (= points) used to simulate the orderbook
    /// ! Price are in quote asset, while quantity are in base asset
    /// Useful:
    /// curl -X GET "https://api.binance.com/api/v3/depth?symbol=ETHUSDC&limit=10"
    /// curl -X GET "https://api.binance.com/api/v3/exchangeInfo?symbol=ETHUSDC" (base = ETH, quote = USDC)
    pub fn depth(&self, limit: Option<u64>) -> OrderbookDepth {
        let limit = match limit {
            Some(limit) => limit,
            None => min(self.bids.len() as u64, self.asks.len() as u64),
        };
        let mut bids_depth = vec![];
        for (x, bid) in self.bids.clone().iter().enumerate() {
            if x == limit as usize {
                break;
            }
            bids_depth.push((bid.average_sell_price, bid.amount));
        }
        let mut asks_depth = vec![];
        for (x, ask) in self.asks.clone().iter().enumerate() {
            if x == limit as usize {
                break;
            }
            let price_in_quote = 1.0 / ask.average_sell_price;
            let amount_in_quote = ask.amount / price_in_quote;
            asks_depth.push((price_in_quote, amount_in_quote));
        }
        // Sort quantities in ascending order
        bids_depth.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        asks_depth.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        let bids_depth_str: Vec<(String, String)> = bids_depth.iter().map(|(price, amount)| (price.to_string(), amount.to_string())).collect();
        let asks_depth_str: Vec<(String, String)> = asks_depth.iter().map(|(price, amount)| (price.to_string(), amount.to_string())).collect();
        OrderbookDepth {
            last_update_id: self.timestamp,
            bids: bids_depth_str,
            asks: asks_depth_str,
        }
    }
    pub fn avgprice(&self) {
        let best_bid = self.mpd_base_to_quote.clone();
    }
}

/// GET /api/v3/trades       --> not possible to implement unless using an external RPC, out of scope ?
/// GET /api/v3/avgPrice     --> implemented

/// ======================================================= Write =======================================================
/// POST /api/v3/order
/// POST /api/v3/order/test
/// GET /api/v3/order

/// ======================================================= Helpers =======================================================
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
