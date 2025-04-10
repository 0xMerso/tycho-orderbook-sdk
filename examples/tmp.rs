use std::collections::HashMap;

// --- Placeholder types for illustration

#[derive(Clone)]
pub struct Network {
    pub rpc: String,
    pub name: String,
    pub chainlink: String,
}

#[derive(Clone)]
pub struct ProtoSimComp {
    // fields omitted for brevity
}

#[derive(Clone)]
pub struct SrzToken {
    pub address: String,
    pub symbol: String,
    pub decimals: u8,
}

#[derive(Clone)]
pub struct OrderbookRequestParams {
    pub tag: String,
    pub point: Option<OrderbookPoint>,
}

#[derive(Clone)]
pub struct OrderbookPoint {
    pub input: String,
    pub amount: f64,
}

/// A dummy TradeResult type used for demonstration.
#[derive(Debug)]
pub struct TradeResult {
    pub detail: String,
}

/// The final Orderbook type returned by simulate.
pub struct Orderbook {
    pub bids: Vec<TradeResult>,
    pub asks: Vec<TradeResult>,
}

// --- Utility function for generating steps (example: exponential progression)
fn exponential(liquidity: f64) -> Vec<f64> {
    let mut steps = Vec::new();
    let mut current = 0.001 * liquidity; // start at 0.1% of liquidity
    while current <= 0.25 * liquidity {
        steps.push(current);
        current *= 2.0;
    }
    steps
}

// --- Step 1: Define a trait that abstracts the "optimize" function logic

pub trait OrderbookSolver: Send + Sync {
    /// Optimize trade simulation for the given parameters.
    ///
    /// # Parameters
    /// - `pcsdata`: A slice of protocol state data.
    /// - `steps`: Precomputed step amounts.
    /// - `eth_usd`: ETH price in USD.
    /// - `gas_price`: Current gas price.
    /// - `from`: The input token.
    /// - `to`: The output token.
    /// - `liquidity`: An adjusted liquidity value.
    /// - `price`: A price used in the simulation.
    /// - `worth`: A weight factor (e.g. token worth in ETH).
    ///
    /// Returns a vector of simulated trade results.
    fn optimize(&self, pcsdata: &[ProtoSimComp], steps: Vec<f64>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, liquidity: f64, price: f64, worth: f64) -> Vec<TradeResult>;
}

// --- Step 2: Provide a default implementation

pub struct DefaultOrderbookSolver;

impl OrderbookSolver for DefaultOrderbookSolver {
    fn optimize(&self, pcsdata: &[ProtoSimComp], steps: Vec<f64>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, liquidity: f64, price: f64, worth: f64) -> Vec<TradeResult> {
        // For demonstration, take the first step (if any) and delegate to a dummy_gradient function.
        let amount = steps.first().copied().unwrap_or(0.0);
        dummy_gradient(amount, pcsdata, from, to, eth_usd, gas_price, price, worth)
    }
}

// A dummy gradient function used by DefaultOrderbookSolver.
fn dummy_gradient(amount: f64, _pcsdata: &[ProtoSimComp], from: &SrzToken, to: &SrzToken, _eth_usd: f64, _gas_price: u128, _price: f64, _worth: f64) -> Vec<TradeResult> {
    println!("Default solver: dummy_gradient called with amount: {:.4} from {} to {}", amount, from.symbol, to.symbol);
    vec![TradeResult {
        detail: format!("Default trade with amount {:.4}", amount),
    }]
}

// --- Step 3: Provide a custom solver implementation

pub struct CustomOrderbookSolver;

impl OrderbookSolver for CustomOrderbookSolver {
    fn optimize(&self, pcsdata: &[ProtoSimComp], steps: Vec<f64>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, liquidity: f64, price: f64, worth: f64) -> Vec<TradeResult> {
        // For custom logic, take the last available step if present.
        let amount = steps.last().copied().unwrap_or(0.0);
        custom_gradient(amount, pcsdata, from, to, eth_usd, gas_price, price, worth)
    }
}

// A custom gradient function used by CustomOrderbookSolver.
fn custom_gradient(amount: f64, _pcsdata: &[ProtoSimComp], from: &SrzToken, to: &SrzToken, _eth_usd: f64, _gas_price: u128, _price: f64, _worth: f64) -> Vec<TradeResult> {
    println!("Custom solver: custom_gradient called with amount: {:.4} from {} to {}", amount, from.symbol, to.symbol);
    vec![TradeResult {
        detail: format!("Custom trade with amount {:.4}", amount),
    }]
}

// --- Step 4: Update the simulate function to accept a generic solver

/// Simulate an orderbook using a solver that implements OrderbookSolver.
/// The function is generic so users can plug in any solver implementation.
pub async fn simulate<S: OrderbookSolver>(
    network: Network,
    pcsdata: Vec<ProtoSimComp>,
    tokens: Vec<SrzToken>,
    body: OrderbookRequestParams,
    solver: S,
    _balances: HashMap<String, f64>,
    _base_worth_eth: f64,
    _quote_worth_eth: f64,
    price_base_to_quote: f64,
    price_quote_to_base: f64,
) -> Orderbook {
    // Dummy ETH price and gas price.
    let eth_usd = 3000.0;
    let gas_price = 100;
    // Assume tokens[0] is the base and tokens[1] is the quote.
    let base = tokens[0].clone();
    let quote = tokens[1].clone();
    // Adjusted liquidity values computed earlier (dummy values here).
    let adjusted_aggb_base = 100.0;
    let adjusted_aggb_quote = 50.0;
    // Compute steps using an exponential function.
    let steps_bid = exponential(adjusted_aggb_base);
    let bids = solver.optimize(
        &pcsdata,
        steps_bid,
        eth_usd,
        gas_price,
        &base,
        &quote,
        adjusted_aggb_base,
        price_base_to_quote,
        1.0, // example worth factor for quote
    );
    let steps_ask = exponential(adjusted_aggb_quote);
    let asks = solver.optimize(
        &pcsdata,
        steps_ask,
        eth_usd,
        gas_price,
        &quote,
        &base,
        adjusted_aggb_quote,
        price_quote_to_base,
        1.0, // example worth factor for base
    );
    Orderbook { bids, asks }
}

// --- Step 5: Example usage in main

#[tokio::main]
async fn main() {
    // Create a dummy network.
    let network = Network {
        rpc: "http://localhost".to_string(),
        name: "LocalNet".to_string(),
        chainlink: "http://chainlink.example".to_string(),
    };
    // Create dummy protocol state data.
    let pcsdata = vec![ProtoSimComp {}];
    // Create two dummy tokens.
    let tokens = vec![
        SrzToken {
            address: "0xBase".to_string(),
            symbol: "BASE".to_string(),
            decimals: 18,
        },
        SrzToken {
            address: "0xQuote".to_string(),
            symbol: "QUOTE".to_string(),
            decimals: 18,
        },
    ];
    // Request parameters.
    let request_params = OrderbookRequestParams {
        tag: "BASE-QUOTE".to_string(),
        point: None,
    };
    let balances = HashMap::new();

    // Example 1: Using the default solver.
    let default_solver = DefaultOrderbookSolver;
    let orderbook_default = simulate(
        network.clone(),
        pcsdata.clone(),
        tokens.clone(),
        request_params.clone(),
        default_solver,
        balances.clone(),
        1.0, // base_worth_eth (dummy)
        1.0, // quote_worth_eth (dummy)
        1.0, // price_base_to_quote
        1.0, // price_quote_to_base
    )
    .await;
    println!("Default solver orderbook -> bids: {:?}, asks: {:?}", orderbook_default.bids, orderbook_default.asks);

    // Example 2: Using the custom solver.
    let custom_solver = CustomOrderbookSolver;
    let orderbook_custom = simulate(
        network,
        pcsdata,
        tokens,
        request_params,
        custom_solver,
        balances,
        1.0, // base_worth_eth
        1.0, // quote_worth_eth
        1.0, // price_base_to_quote
        1.0, // price_quote_to_base
    )
    .await;
    println!("Custom solver orderbook -> bids: {:?}, asks: {:?}", orderbook_custom.bids, orderbook_custom.asks);
}
