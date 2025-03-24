use std::{
    collections::HashMap,
    fmt::{self, Display},
    sync::Arc,
};

use alloy::{network::TransactionBuilder, rpc::types::TransactionRequest};
use alloy_primitives::TxKind;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use utoipa::ToSchema;

alloy::sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    IERC20,
    "src/shd/utils/abis/IERC20.json"
);

alloy::sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    IBalancer2Vault,
    "src/shd/utils/abis/Balancer2Vault.json"
);

#[derive(Debug, Clone, utoipa::ToSchema)]
pub struct EnvConfig {
    pub testing: bool,
    // pub tycho_url: String,
    pub tycho_api_key: String,
    pub network: String,
    pub pvkey: String,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Network {
    #[schema(example = "ethereum")]
    pub name: String,
    #[schema(example = "1")]
    pub chainid: u64,
    #[schema(example = "0x")]
    pub eth: String,
    #[schema(example = "0x")]
    pub usdc: String,
    #[schema(example = "0x")]
    pub exotic: String,
    #[schema(example = "0x")]
    pub wbtc: String,
    #[schema(example = "0x")]
    pub dai: String,
    #[schema(example = "0x")]
    pub usdt: String,

    #[schema(example = "https://rpc.payload.de")]
    pub rpc: String,
    #[schema(example = "https://etherscan.io/")]
    pub exp: String,
    #[schema(example = "true")]
    pub enabled: bool,
    #[schema(example = "http://tycho-beta.propellerheads.xyz")]
    pub tycho: String,
    #[schema(example = "4242")]
    pub port: u64,
    #[schema(example = "0x")]
    pub balancer: String,

    #[schema(example = "0x")]
    pub permit2: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolData {
    pub liquidity: u128,
    pub sqrt_price_x96: u128,
    pub tick: u128,
    pub ticks: Vec<TickData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickData {
    pub tick: i32,
    pub net_liquidity: u128,
    pub price0to1: f64,
    pub price1to0: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookSimu {
    pub pool: String,
    pub simulations: Vec<BestSwap>,
    pub spot_sprice: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestSwap {
    pub formatted_in: String,
    pub formatted_out: String,
    pub forward_price: f64,
    pub reverse_price: f64,
}

pub enum TychoSupportedProtocol {
    Pancakeswap,
    Sushiswap,
    UniswapV2,
    UniswapV3,
    UniswapV4,
    BalancerV2,
    Curve,
}

impl ToString for TychoSupportedProtocol {
    fn to_string(&self) -> String {
        match self {
            TychoSupportedProtocol::Pancakeswap => "pancakeswap_v2".to_string(),
            TychoSupportedProtocol::Sushiswap => "sushiswap_v2".to_string(),
            TychoSupportedProtocol::UniswapV2 => "uniswap_v2".to_string(),
            TychoSupportedProtocol::UniswapV3 => "uniswap_v3".to_string(),
            TychoSupportedProtocol::UniswapV4 => "uniswap_v4".to_string(),
            TychoSupportedProtocol::BalancerV2 => "vm:balancer_v2".to_string(),
            TychoSupportedProtocol::Curve => "vm:curve".to_string(),
        }
    }
}

// Impl vectorize for TychoSupportedProtocol
impl TychoSupportedProtocol {
    pub fn vectorize() -> Vec<String> {
        vec![
            TychoSupportedProtocol::Pancakeswap.to_string(),
            TychoSupportedProtocol::Sushiswap.to_string(),
            TychoSupportedProtocol::UniswapV2.to_string(),
            TychoSupportedProtocol::UniswapV3.to_string(),
            TychoSupportedProtocol::UniswapV4.to_string(),
            TychoSupportedProtocol::BalancerV2.to_string(),
            TychoSupportedProtocol::Curve.to_string(),
        ]
    }
}

pub enum AmmType {
    Pancakeswap,
    Sushiswap,
    UniswapV2,
    UniswapV3,
    UniswapV4,
    Balancer,
    Curve,
}

impl ToString for AmmType {
    fn to_string(&self) -> String {
        match self {
            AmmType::Pancakeswap => "pancakeswap_v2_pool".to_string(),
            AmmType::Sushiswap => "sushiswap_v2_pool".to_string(),
            AmmType::UniswapV2 => "uniswap_v2_pool".to_string(),
            AmmType::UniswapV3 => "uniswap_v3_pool".to_string(),
            AmmType::UniswapV4 => "uniswap_v4_pool".to_string(),
            AmmType::Balancer => "balancer_v2_pool".to_string(),
            AmmType::Curve => "curve_pool".to_string(), // ?
        }
    }
}

impl From<&str> for AmmType {
    fn from(s: &str) -> Self {
        match s {
            "pancakeswap_v2_pool" => AmmType::Pancakeswap,
            "sushiswap_v2_pool" => AmmType::Sushiswap,
            "uniswap_v2_pool" => AmmType::UniswapV2,
            "uniswap_v3_pool" => AmmType::UniswapV3,
            "uniswap_v4_pool" => AmmType::UniswapV4,
            "balancer_v2_pool" => AmmType::Balancer,
            "curve_pool" => AmmType::Curve, // ?
            _ => panic!("Unknown AMM type"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SyncState {
    Down = 1,
    Launching = 2,
    Syncing = 3,
    Running = 4,
    Error = 5,
}

impl Display for SyncState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SyncState::Down => write!(f, "Down"),
            SyncState::Launching => write!(f, "Launching"),
            SyncState::Syncing => write!(f, "Syncing"),
            SyncState::Running => write!(f, "Running"),
            SyncState::Error => write!(f, "Error"),
        }
    }
}

// ======== Shared Data per tasks ========

use tycho_simulation::protocol::{models::ProtocolComponent, state::ProtocolSim};

use super::data::fmt::{SrzProtocolComponent, SrzToken};

pub type SharedTychoStreamState = Arc<RwLock<TychoStreamState>>;

pub struct TychoStreamState {
    // Maps a network name to its ProtocolSim instance.
    pub protosims: HashMap<String, Box<dyn ProtocolSim>>,
    // pub states: HashMap<String, String>,
    // Maps a network name to its new ProtocolComponent.
    // pub components: HashMap<String, ProtocolComponent>,
    pub components: HashMap<String, ProtocolComponent>,
    pub balances: HashMap<String, HashMap<String, u128>>, // FuLL Lowercase
}

pub type ChainCore = tycho_core::dto::Chain;
pub type ChainSimCore = tycho_simulation::tycho_core::dto::Chain;
pub type ChainSimu = tycho_simulation::evm::tycho_models::Chain;

pub fn chain(name: String) -> Option<(ChainCore, ChainSimCore, ChainSimu)> {
    match name.as_str() {
        "ethereum" => Some((ChainCore::Ethereum, ChainSimCore::Ethereum, ChainSimu::Ethereum)),
        "arbitrum" => Some((ChainCore::Arbitrum, ChainSimCore::Arbitrum, ChainSimu::Arbitrum)),
        "base" => Some((ChainCore::Base, ChainSimCore::Base, ChainSimu::Base)),
        _ => {
            log::error!("Unknown chain: {}", name);
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
            log::error!("Unknown chain: {}", name);
            600
        }
    }
}

// ================================================================ API ================================================================

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct OrderbookRequestBody {
    pub tag: String, // Pair uniq identifier: token0-token1
    pub sps: Option<SinglePointSimulation>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct SinglePointSimulation {
    pub input: String, // 0to1 if input = token0
    pub amount: f64,
}

#[derive(Debug, Deserialize)]
pub struct PairResponse {
    pub components: String,   // Must have same size
    pub states: Option<bool>, // Must have same size
}

#[derive(Clone, Debug)]
pub struct ProtoTychoState {
    pub component: SrzProtocolComponent,
    pub protosim: Box<dyn ProtocolSim>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairLiquidityBook {
    pub from: SrzToken,
    pub to: SrzToken,
    pub orderbooks: Vec<LiquidityPoolBook>,
}

/// Whatever the protocol is, it must comply with this struct
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityPoolBook {
    pub address: String,  // Proto/PooL address
    pub protocol: String, // Component Protocol name
    // pub z0to1: bool,        // Zero to One as Uniswap expresses it
    pub concentrated: bool, // Concentrated liquidity
    pub fee: f64,           // Fee according to ProtoSim
    pub price: f64,         // Price Spot (0 to 1 if z0to1 is true)
    pub reserves: Vec<f64>, // reserves[0], reserves[1]
    pub tick: i32,          // Current tick
    pub spacing: u64,       // Tick spacing
    pub bids: Vec<LiquidityTickAmounts>,
    pub asks: Vec<LiquidityTickAmounts>,
}

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct TradeResult {
//     pub input: BigUint,             // e.g. 100 (meaning 100 ETH)
//     pub output: BigUint,            // in token_out human–readable units
//     pub distribution: Vec<BigUint>, // percentage distribution per pool (0–100)
//     pub ratio: BigUint,             // output per unit input (human–readable)
// }

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TradeResult {
    #[schema(example = "1.0")]
    pub amount: f64, // e.g. 100 (meaning 100 ETH)
    #[schema(example = "2000.0")]
    pub output: f64, // in token_out human–readable units
    #[schema(example = "[0.42, 0.37, 0.21]")]
    pub distribution: Vec<f64>, // percentage distribution per pool (0–100)
    #[schema(example = "[42000, 37000, 77000]")]
    pub gas_costs: Vec<u128>,
    #[schema(example = "[0.42, 0.37, 0.77]")]
    pub gas_costs_usd: Vec<f64>,
    // #[schema(example = "[1.77, 2.42, 2.37]")]
    // pub gas_costs_output: Vec<f64>,
    #[schema(example = "0.0005")]
    pub ratio: f64, // output per unit input (human–readable)
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MidPriceData {
    pub best_ask: f64,
    pub best_bid: f64,
    pub mid: f64,
    pub spread: f64,
    pub spread_pct: f64,
    // pub inverse_price0t1: f64,
    // pub inverse_price1t0: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PairSimulatedOrderbook {
    pub token0: SrzToken,
    pub token1: SrzToken,
    pub prices0to1: Vec<f64>,
    pub prices1to0: Vec<f64>,
    pub trades0to1: Vec<TradeResult>,
    pub trades1to0: Vec<TradeResult>,
    pub aggt0lqdty: Vec<f64>,
    pub aggt1lqdty: Vec<f64>,
    pub pools: Vec<SrzProtocolComponent>,
    pub eth_usd: f64,
    // pub best0to1: TradeResult, // Bid of Ask, it's a pov
    // pub best1to0: TradeResult, // Bid of Ask, it's a pov
    pub mpd0to1: MidPriceData,
    pub mpd1to0: MidPriceData,
    pub t0_worth_eth: f64, // One unit, multi-hop spot_price, needed to value the TVL and other stuff
    pub t1_worth_eth: f64, // One unit, multi-hop spot_price, needed to value the TVL and other stuff
}

// =================================================================================== EXECUTION =======================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExecutionRequest {
    pub sender: String,
    pub tag: String,
    pub input: SrzToken,
    pub output: SrzToken,
    pub amount_in: f64,
    pub expected_amount_out: f64,
    pub distribution: Vec<f64>, // Percentage distribution per pool (0–100)
    pub components: Vec<SrzProtocolComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExecutionContext {
    pub router: String,
    pub sender: String,
    pub fork: bool,
    pub request: ExecutionRequest,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExecutionPayload {
    // pub hash: String,
    // pub success: bool,
    // pub reason: bool,
    pub approve: SrzTransactionRequest,
    pub swap: SrzTransactionRequest,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SrzTransactionRequest {
    pub from: String,                   // Option<Address>,
    pub to: String,                     // Option<TxKind>,
    pub gas_price: u128,                // Option<u128>,
    pub max_fee_per_gas: u128,          // Option<u128>,
    pub max_priority_fee_per_gas: u128, // Option<u128>,
    pub max_fee_per_blob_gas: u128,     // Option<u128>,
    pub gas: u128,                      // Option<u128>,
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
            value: value,
            input: input.clone(),
            nonce: nonce,
            chain_id: chain_id,
        }
    }
}

/// ====================================================================================================================================================================================================
/// Ticks and Liquidity

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
