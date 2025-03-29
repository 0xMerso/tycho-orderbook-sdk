use std::cmp::min;

use crate::shd::types::{ExchangeInfo, Network, Orderbook, OrderbookDepth};

/// Implement conversion from Orderbook to a standard Orderbook format like Binance
/// Binance: https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-endpoints

/// ======================================================= Read =======================================================
/// GET /api/v3/exchangeInfo --> implemented ✅
/// GET /api/v3/depth        --> implemented ✅
/// GET /api/v3/trades       --> not implementable
/// GET /api/v3/avgPrice     --> not implementable

impl Orderbook {
    /// Onchain liquidity is not splitted by orders like in a traditional orderbook, the implementation is a bit different than the one from Binance or other exchanges

    /// Get the orderbook depth (depends on the amounts (= points) used to simulate the orderbook)
    /// Price are in quote asset, while quantity are in base asset
    /// See https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-endpoints#terminology
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

    /// Get the exchange info
    pub fn exchange(&self) -> ExchangeInfo {
        ExchangeInfo {
            timezone: "UTC".to_string(),
            base: self.base.clone(),
            quote: self.quote.clone(),
            components: self.pools.clone(),
            order_types: vec!["MARKET".to_string()],
        }
    }

    /// ======================================================= Write =======================================================

    /// POST /api/v3/order
    pub async fn execute_trade(&self) {
        log::info!("execute_trade");
    }

    /// POST /api/v3/order/test
    pub async fn simulate_trade(&self) {
        log::info!("simulate_trade");
    }
}
