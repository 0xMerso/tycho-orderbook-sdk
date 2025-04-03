use crate::{
    core::exec,
    types::{ExchangeInfo, ExecutedPayload, ExecutionRequest, Network, Orderbook, OrderbookDepth, PayloadToExecute},
};
use async_trait::async_trait;
use std::cmp::min;
use tycho_simulation::protocol::models::ProtocolComponent;

/// Adapters are customized interfaces implemented for specific needs on the Orderbook struct, such as the reproduction of the exchange's orderbook format.
/// The default adapter is designed to match as much as possible the Binance standard.

/// Implement conversion from Orderbook to a standard Orderbook format.
/// Binance: https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-endpoints
/// GET /api/v3/exchangeInfo --> implemented
/// GET /api/v3/depth        --> implemented
/// GET /api/v3/trades       --> not relevant
/// GET /api/v3/avgPrice     --> not relevant

#[async_trait]
pub trait DefaultOrderBookAdapter: Send + Sync {
    /// Returns orderbook depth snapshot (limited if specified).
    fn depth(&self, limit: Option<u64>) -> OrderbookDepth;

    /// Returns static metadata (e.g., name, symbols, fees).
    fn info(&self) -> ExchangeInfo;

    /// Create a trade payload (or sends the order to the exchange).
    async fn create(&self, network: Network, request: ExecutionRequest, components: Vec<ProtocolComponent>, pk: Option<String>) -> Result<PayloadToExecute, String>; // (&mut self, side: Side, quantity: f64, price: f64);

    /// Sends the payload of transactions (approve, swap, )
    async fn send(&self, network: Network, payload: PayloadToExecute, pk: Option<String>) -> ExecutedPayload;
}

#[async_trait]
impl DefaultOrderBookAdapter for Orderbook {
    /// Get the orderbook depth (depends on the amounts (= points) used to simulate the orderbook)
    /// Price are in quote asset, while quantity are in base asset
    /// See https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-endpoints#terminology
    /// curl -X GET "https://api.binance.com/api/v3/depth?symbol=ETHUSDC&limit=10"
    /// curl -X GET "https://api.binance.com/api/v3/exchangeInfo?symbol=ETHUSDC" (base = ETH, quote = USDC)
    fn depth(&self, limit: Option<u64>) -> OrderbookDepth {
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
        // let bids_depth_str: Vec<(String, String)> = bids_depth.iter().map(|(price, amount)| (price.to_string(), amount.to_string())).collect();
        // let asks_depth_str: Vec<(String, String)> = asks_depth.iter().map(|(price, amount)| (price.to_string(), amount.to_string())).collect();
        OrderbookDepth {
            last_update_id: self.timestamp,
            bids: bids_depth,
            asks: asks_depth,
        }
    }

    /// Get the exchange info
    fn info(&self) -> ExchangeInfo {
        ExchangeInfo {
            timezone: "UTC".to_string(),
            base: self.base.clone(),
            quote: self.quote.clone(),
            components: self.pools.clone(),
            order_types: vec!["MARKET".to_string()],
        }
    }

    /// POST /api/v3/order
    async fn create(&self, network: Network, request: ExecutionRequest, components: Vec<ProtocolComponent>, pk: Option<String>) -> Result<PayloadToExecute, String> {
        match exec::build(network.clone(), request.clone(), components.clone(), pk.clone()).await {
            Ok(payload) => Ok(payload),
            Err(e) => {
                tracing::error!("Error executing order: {}", e);
                Err(e)
            }
        }
    }

    /// Send the payload of transactions
    async fn send(&self, network: Network, payload: PayloadToExecute, pk: Option<String>) -> ExecutedPayload {
        exec::broadcast(network.clone(), payload.clone(), pk).await
    }
}
