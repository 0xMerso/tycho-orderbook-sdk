use std::{collections::HashMap, sync::Arc};

use alloy::rpc::types::TransactionRequest;
use alloy_primitives::TxKind;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use utoipa::ToSchema;

use super::data::fmt::{SrzProtocolComponent, SrzToken};
use tycho_simulation::evm::decoder::StreamDecodeError;
use tycho_simulation::protocol::{models::ProtocolComponent, state::ProtocolSim};

pub type SharedTychoStreamState = Arc<RwLock<TychoStreamState>>;

alloy::sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    IERC20,
    "src/utils/abis/IERC20.json"
);

alloy::sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    IBalancer2Vault,
    "src/utils/abis/Balancer2Vault.json"
);

alloy::sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    IChainLinkPF,
    "src/utils/abis/Chainlink.json"
);

#[derive(Default, Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Network {
    #[schema(example = "ethereum")]
    pub name: String,
    #[schema(example = "1")]
    pub chainid: u64,
    #[schema(example = "0x")]
    pub eth: String,
    #[schema(example = "https://rpc.payload.de")]
    pub rpc: String,
    #[schema(example = "https://etherscan.io/")]
    pub exp: String,
    #[schema(example = "http://tycho-beta.propellerheads.xyz")]
    pub tycho: String,
    #[schema(example = "4242")]
    pub port: u64,
    #[schema(example = "0x")]
    pub router: String,
    #[schema(example = "0x")]
    pub permit2: String,
    #[schema(example = "Symbol")]
    pub tag: String,
    #[schema(example = "0x")]
    pub chainlink: String,
}

/// Tycho protocol, used to configure ProtocolStreamBuilder
pub enum TychoSupportedProtocol {
    PancakeswapV2,
    PancakeswapV3,
    Sushiswap,
    UniswapV2,
    UniswapV3,
    UniswapV4,
    EkuboV2,
    BalancerV2,
    Curve,
}

#[allow(clippy::to_string_trait_impl)]
impl ToString for TychoSupportedProtocol {
    fn to_string(&self) -> String {
        match self {
            TychoSupportedProtocol::PancakeswapV2 => "pancakeswap_v2".to_string(),
            TychoSupportedProtocol::PancakeswapV3 => "pancakeswap_v3".to_string(),
            TychoSupportedProtocol::Sushiswap => "sushiswap_v2".to_string(),
            TychoSupportedProtocol::UniswapV2 => "uniswap_v2".to_string(),
            TychoSupportedProtocol::UniswapV3 => "uniswap_v3".to_string(),
            TychoSupportedProtocol::UniswapV4 => "uniswap_v4".to_string(),
            TychoSupportedProtocol::EkuboV2 => "ekubo_v2".to_string(),
            TychoSupportedProtocol::BalancerV2 => "vm:balancer_v2".to_string(),
            TychoSupportedProtocol::Curve => "vm:curve".to_string(),
        }
    }
}

// Impl vectorize for TychoSupportedProtocol
impl TychoSupportedProtocol {
    pub fn vectorize() -> Vec<String> {
        vec![
            TychoSupportedProtocol::PancakeswapV2.to_string(),
            TychoSupportedProtocol::PancakeswapV3.to_string(),
            TychoSupportedProtocol::Sushiswap.to_string(),
            TychoSupportedProtocol::UniswapV2.to_string(),
            TychoSupportedProtocol::UniswapV3.to_string(),
            TychoSupportedProtocol::UniswapV4.to_string(),
            TychoSupportedProtocol::EkuboV2.to_string(),
            TychoSupportedProtocol::BalancerV2.to_string(),
            TychoSupportedProtocol::Curve.to_string(),
        ]
    }
}

/// Tycho Protocol type name, used to add exchanges
pub enum AmmType {
    PancakeswapV2,
    PancakeswapV3,
    Sushiswap,
    UniswapV2,
    UniswapV3,
    UniswapV4,
    EkuboV2,
    Balancer,
    Curve,
}

#[allow(clippy::to_string_trait_impl)]
impl ToString for AmmType {
    fn to_string(&self) -> String {
        match self {
            AmmType::PancakeswapV2 => "pancakeswap_v2_pool".to_string(),
            AmmType::PancakeswapV3 => "pancakeswap_v3_pool".to_string(),
            AmmType::Sushiswap => "sushiswap_v2_pool".to_string(),
            AmmType::UniswapV2 => "uniswap_v2_pool".to_string(),
            AmmType::UniswapV3 => "uniswap_v3_pool".to_string(),
            AmmType::UniswapV4 => "uniswap_v4_pool".to_string(),
            AmmType::EkuboV2 => "ekubo_v2_pool".to_string(),
            AmmType::Balancer => "balancer_v2_pool".to_string(),
            AmmType::Curve => "curve_pool".to_string(), // ?
        }
    }
}

impl From<&str> for AmmType {
    fn from(s: &str) -> Self {
        match s {
            "pancakeswap_v2_pool" => AmmType::PancakeswapV2,
            "pancakeswap_v3_pool" => AmmType::PancakeswapV3,
            "sushiswap_v2_pool" => AmmType::Sushiswap,
            "uniswap_v2_pool" => AmmType::UniswapV2,
            "uniswap_v3_pool" => AmmType::UniswapV3,
            "uniswap_v4_pool" => AmmType::UniswapV4,
            "balancer_v2_pool" => AmmType::Balancer,
            "curve_pool" => AmmType::Curve,      // ?
            "ekubo_v2_pool" => AmmType::EkuboV2, // ?
            _ => panic!("Unknown AMM type"),
        }
    }
}

// =================================================================================== EXECUTION =======================================================================================================

#[derive(Default, Debug, Clone)]
pub struct ExecTxResult {
    pub sent: bool,
    pub status: bool,
    pub hash: String,
    pub error: Option<String>,
}

#[derive(Default, Debug, Clone)]
pub struct ExecutedPayload {
    pub approve: ExecTxResult,
    pub swap: ExecTxResult,
}

/// Result of the execution
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PayloadToExecute {
    pub approve: TransactionRequest,
    pub swap: TransactionRequest,
}

/// Execution request, used to simulate a trade
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExecutionRequest {
    pub sender: String,
    pub tag: String,
    pub input: SrzToken,
    pub output: SrzToken,
    pub amount: f64,
    pub expected: f64,
    pub distribution: Vec<f64>, // Percentage distribution per pool (0–100)
    pub components: Vec<SrzProtocolComponent>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SrzExecutionPayload {
    pub swap: SrzTransactionRequest,
    pub approve: SrzTransactionRequest,
}

/// Transaction request, serialized for the client (srz = serialized)
#[derive(Default, Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SrzTransactionRequest {
    pub from: String,                   // Option<Address>,
    pub to: String,                     // Option<TxKind>,
    pub gas_price: u128,                // Option<u128>,
    pub max_fee_per_gas: u128,          // Option<u128>,
    pub max_priority_fee_per_gas: u128, // Option<u128>,
    pub max_fee_per_blob_gas: u128,     // Option<u128>,
    pub gas: u64,                       // Option<u128>,
    pub value: u128,                    // Option<U256>,
    pub input: String,                  // TransactionInput,
    pub nonce: u128,                    // Option<u64>,
    pub chain_id: u128,                 // Option<ChainId>,
}

// Convert Alloy TransactionRequest to a client friendly format
impl From<TransactionRequest> for SrzTransactionRequest {
    fn from(tr: TransactionRequest) -> Self {
        let to = tr.to.unwrap_or_default();
        let to = match to {
            TxKind::Call(addr) => addr.to_string(),
            _ => "".to_string(),
        };
        let value = tr.value.unwrap_or_default().to_string().parse::<u128>().unwrap_or_default();
        let nonce = tr.nonce.unwrap_or_default().to_string().parse::<u128>().unwrap_or_default();
        let chain_id = tr.chain_id.unwrap_or_default().to_string().parse::<u128>().unwrap_or_default();
        let input = tr.input.input.unwrap_or_default().to_string();
        SrzTransactionRequest {
            from: tr.from.map(|addr| addr.to_string()).unwrap_or_default(),
            to: to.to_string(),
            gas_price: tr.gas_price.unwrap_or(0),
            max_fee_per_gas: tr.max_fee_per_gas.unwrap_or(0),
            max_priority_fee_per_gas: tr.max_priority_fee_per_gas.unwrap_or(0),
            max_fee_per_blob_gas: tr.max_fee_per_blob_gas.unwrap_or(0),
            gas: tr.gas.unwrap_or(0),
            value,
            input: input.clone(),
            nonce,
            chain_id,
        }
    }
}

// =================================================================================== Liquidity types =======================================================================================================

#[derive(Debug, Clone)]
pub struct TickDataRange {
    pub tick_lower: i32,
    pub sqrt_price_lower: u128,
    pub tick_upper: i32,
    pub sqrt_price_upper: u128,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityTickAmounts {
    pub index: i32,
    pub amount0: f64,
    pub amount1: f64,
    pub p0to1: f64,
    pub p1to0: f64,
}

#[derive(Default, Debug, Clone)]
pub struct SummedLiquidity {
    pub amount0: f64,
    pub amount1: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct IncrementationSegment {
    pub start: f64,
    pub end: f64,
    pub step: f64,
}

#[derive(Debug)]
pub struct PairSimuIncrementConfig {
    pub segments: Vec<IncrementationSegment>,
}

// =================================================================================== Tycho & Protocols State =======================================================================================================

/// Due to library conflicts, we need to redefine the Chain type depending the use case, hence the following aliases.
pub type ChainCommon = tycho_common::dto::Chain;
pub type ChainSimCore = tycho_simulation::tycho_core::dto::Chain;
pub type ChainSimu = tycho_simulation::evm::tycho_models::Chain;

/// Return the chains types for a given network name
pub fn chain(name: String) -> Option<(ChainCommon, ChainSimCore, ChainSimu)> {
    match name.as_str() {
        "ethereum" => Some((ChainCommon::Ethereum, ChainSimCore::Ethereum, ChainSimu::Ethereum)),
        "arbitrum" => Some((ChainCommon::Arbitrum, ChainSimCore::Arbitrum, ChainSimu::Arbitrum)),
        "base" => Some((ChainCommon::Base, ChainSimCore::Base, ChainSimu::Base)),
        _ => {
            tracing::error!("Unknown chain: {}", name);
            None
        }
    }
}

/// Overwriting - Returns the default block time and timeout values for the given blockchain network.
pub fn chain_timing(name: String) -> u64 {
    match name.as_str() {
        "ethereum" => 600,
        "starknet" => 30,
        "zksync" => 1,
        "arbitrum" => 1,
        "base" => 10,
        _ => {
            tracing::error!("Unknown chain: {}", name);
            600
        }
    }
}

// =================================================================================== Core SDK =======================================================================================================

/// Orderbook Provider Event
#[derive(Debug)]
pub enum OrderbookEvent {
    /// Event when the stream is initialised = connected to Tycho
    Initialised(u64),
    /// Emited when a new header is received, with components ID that have changed
    NewHeader(u64, Vec<String>),
    /// Stream Error
    Error(StreamDecodeError),
}

/// Tycho Stream Data, stored in a Mutex/Arc for shared access between the SDK stream and the client or API.
pub struct TychoStreamState {
    // ProtocolSim instances, indexed by their unique identifier. Impossible to store elsewhere than memory
    pub protosims: HashMap<String, Box<dyn ProtocolSim>>,
    // Components instances, indexed by their unique identifier. Serialised and stored in Redis
    pub components: HashMap<String, ProtocolComponent>,
    // Indicates whether the ProtocolStreamBuilder has been initialised (true if first stream has been received and saved)
    pub initialised: bool,
}

/// One component of the Tycho protocol, with his simulation instance
#[derive(Clone, Debug)]
pub struct ProtoSimComp {
    pub component: SrzProtocolComponent,
    pub protosim: Box<dyn ProtocolSim>,
}

/// Orderbook request params used to build a orderbook for a given pair
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct OrderbookRequestParams {
    /// Pair uniq identifier: token0-token1 => base-quote
    /// Example: ETH/USDC
    /// - Bid = buy orders for the base asset (ETH) priced in USDC
    /// - Ask = sell orders for the base asset (ETH) priced in USDC.
    #[schema(example = "0xETH-0xUSDC")]
    pub tag: String,
    /// Optional single point simulation, used to simulate 1 trade only
    pub point: Option<SinglePointSimulation>,
}

/// Orderbook query, but for one point (= 1 trade = 1 amount in)
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct SinglePointSimulation {
    // Address of the input token
    #[schema(example = "0xETH")]
    pub input: String,
    // Divided by input decimals
    #[schema(example = "10")]
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TradeResult {
    // e.g. 100 (meaning 100 ETH of input)
    #[schema(example = "1.0")]
    pub amount: f64,

    // In token_out human–readable units
    #[schema(example = "2000.0")]
    pub output: f64,

    // Percentage distribution (amount in) per pool (0–100)
    #[schema(example = "[0.42, 0.37, 0.21]")]
    pub distribution: Vec<f64>,

    // Percentage received per pool
    #[schema(example = "[0.42, 0.37, 0.21]")]
    pub distributed: Vec<f64>,

    // Gas units used
    #[schema(example = "[42000, 37000, 77000]")]
    pub gas_costs: Vec<u128>,

    // Gas costs in USD depending the pool
    #[schema(example = "[0.42, 0.37, 0.77]")]
    pub gas_costs_usd: Vec<f64>,

    // output per unit input (human–readable)
    #[schema(example = "0.0005")]
    pub average_sell_price: f64,

    // Price impact of the trade (0–1). In absolute value, bc cannot be positive.
    #[schema(example = "0.05")]
    pub price_impact: f64,
}

/// Orderbook data used to compute spread, and other metrics
#[derive(Default, Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MidPriceData {
    pub ask: f64,
    pub bid: f64,
    pub mid: f64,
    pub spread: f64,
    pub spread_pct: f64,
    pub amount: f64,
    pub received: f64,
    pub distribution: Vec<f64>,
}

/// FuLL orderbook data response. Key struct of the SDK
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Orderbook {
    pub tag: String,
    /// Block number of the orderbook, state at which the orderbook was built
    pub block: u64,
    /// When the orderbook started to be built (seconds since epoch)
    pub timestamp: u64,
    /// Token0. Input and output token
    pub base: SrzToken,
    /// Token1. Output then output token
    pub quote: SrzToken,
    /// Prices from token0 to token1. Always divided by decimals
    pub prices_base_to_quote: Vec<f64>,
    /// Prices from token1 to token0. Always divided by decimals
    pub prices_quote_to_base: Vec<f64>,
    /// Array of resulat for the optimal single hop route
    pub bids: Vec<TradeResult>,
    /// Array of resulat for the optimal single hop route
    pub asks: Vec<TradeResult>,
    /// Cumulated liquidity for base, always divided by decimals, combining all pools/components
    pub base_lqdty: Vec<f64>,
    /// Cumulated liquidity for quote, always divided by decimals, combining all pools/components
    pub quote_lqdty: Vec<f64>,
    /// All components used to build the orderbook (= pools that include both token0 and token1)
    pub pools: Vec<SrzProtocolComponent>,
    /// Current value of ETH in USD
    pub eth_usd: f64,
    /// Mid price data for token0 to token1
    pub mpd_base_to_quote: MidPriceData,
    /// Mid price data for token1 to token0
    pub mpd_quote_to_base: MidPriceData,
    /// One unit, multi-hop spot_price, needed to value the TVL and other stuff
    pub base_worth_eth: f64,
    /// One unit, multi-hop spot_price, needed to value the TVL and other stuff
    pub quote_worth_eth: f64,
    // The TVL value in USD for the base token
    pub aggregated_balance_base_worth_usd: f64,
    // The TVL value in USD for the quote token
    pub aggregated_balance_quote_worth_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookDepth {
    pub last_update_id: u64,
    pub bids: Vec<(f64, f64)>,
    pub asks: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeInfo {
    pub timezone: String,
    pub base: SrzToken,
    pub quote: SrzToken,
    pub order_types: Vec<String>,
    pub components: Vec<SrzProtocolComponent>,
}
/// ================================================================================= External =======================================================================================================

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct CoinGeckoResponse {
    pub ethereum: CryptoPrice,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct CryptoPrice {
    pub usd: f64,
}
