use crate::shd::{
    core::amms::get_pool_balances,
    data::fmt::{SrzEVMPoolState, SrzProtocolComponent, SrzToken, SrzUniswapV2State, SrzUniswapV3State, SrzUniswapV4State},
    maths::{self},
    types::PairQuery,
};

use crate::shd::types::LiquidityPoolBook;

use alloy::providers::ProviderBuilder;
use tycho_simulation::{
    evm::{
        engine_db::tycho_db::PreCachedDB,
        protocol::{uniswap_v2::state::UniswapV2State, uniswap_v3::state::UniswapV3State, uniswap_v4::state::UniswapV4State, vm::state::EVMPoolState},
    },
    models::Token,
    protocol::state::ProtocolSim,
};

use crate::shd::types::{AmmType, Network, PairLiquidityBook, ProtoTychoState};

pub trait ToLiquidityBook {
    fn structurate(&self, component: SrzProtocolComponent, tks: Vec<SrzToken>, query: PairQuery, fee: f64, price: f64, poolb0: f64, poolb1: f64) -> LiquidityPoolBook;
}

// ========================================================================= NOT CONCENTRATED ===========================================================================================================

impl ToLiquidityBook for SrzUniswapV2State {
    fn structurate(&self, component: SrzProtocolComponent, tks: Vec<SrzToken>, query: PairQuery, fee: f64, price: f64, poolb0: f64, poolb1: f64) -> LiquidityPoolBook {
        LiquidityPoolBook {
            address: component.id.clone().to_lowercase(),
            protocol: component.protocol_type_name.clone(),
            concentrated: false,
            fee,
            price,
            reserves: vec![poolb0, poolb1],
            bids: vec![],
            asks: vec![],
            tick: 0,
            spacing: 0,
        }
    }
}

impl ToLiquidityBook for SrzEVMPoolState {
    fn structurate(&self, component: SrzProtocolComponent, tks: Vec<SrzToken>, query: PairQuery, fee: f64, price: f64, poolb0: f64, poolb1: f64) -> LiquidityPoolBook {
        LiquidityPoolBook {
            address: component.id.clone().to_lowercase(),
            protocol: component.protocol_type_name.clone(),
            concentrated: false,
            fee,
            price,
            reserves: vec![poolb0, poolb1],
            bids: vec![],
            asks: vec![],
            tick: 0,
            spacing: 0,
        }
    }
}

// ========================================================================= CONCENTRATED ==============================================================================================================

impl ToLiquidityBook for SrzUniswapV3State {
    fn structurate(&self, component: SrzProtocolComponent, tks: Vec<SrzToken>, query: PairQuery, fee: f64, price: f64, poolb0: f64, poolb1: f64) -> LiquidityPoolBook {
        // CCT
        let spacing = self.ticks.tick_spacing as i32;
        maths::ticks::ticks_liquidity(self.liquidity as i128, self.tick, spacing, &self.ticks.clone(), tks[0].clone(), tks[1].clone(), false);
        let tick_data_range = maths::ticks::compute_tick_data(self.tick, self.ticks.tick_spacing as i32);
        // let (p0to1, p1to0) = maths::ticks::tick_to_prices(tick_data_range.tick_lower, tks[0].decimals as u8, tks[1].decimals as u8);
        let (p0to1, p1to0) = maths::ticks::tick_to_prices(self.tick, tks[0].decimals as u8, tks[1].decimals as u8);
        // let (p0to1, p1to0) = maths::ticks::tick_to_prices(tick_data_range.tick_upper, tks[0].decimals as u8, tks[1].decimals as u8);
        log::info!("ToLiquidityBook: SrzUniswapV3State: Analysing current tick.");
        let current_tick_amounts = maths::ticks::derive(
            self.liquidity as i128,
            self.sqrt_price.to_string().parse::<f64>().unwrap(), // At current tick, not the boundaries !
            tick_data_range.tick_lower,
            tick_data_range.tick_upper,
            tks[0].clone(),
            tks[1].clone(),
            true,
        );
        log::info!("ToLiquidityBook: SrzUniswapV3State: Analysing each tick individually.");
        let mut ticks_list = self.ticks.clone();
        ticks_list.ticks.sort_by(|a, b| a.index.cmp(&b.index));
        let ticks_amounts = maths::ticks::ticks_liquidity(self.liquidity as i128, self.tick, spacing, &ticks_list.clone(), tks[0].clone(), tks[1].clone(), false);
        let (mut bids, asks) = maths::ticks::filter_and_classify_ticks(ticks_amounts, self.tick, tick_data_range.tick_lower, p0to1, p1to0);
        bids.push(current_tick_amounts.clone());
        LiquidityPoolBook {
            address: component.id.clone().to_lowercase(),
            protocol: component.protocol_type_name.clone(),
            concentrated: true,
            fee,
            price,
            reserves: vec![poolb0, poolb1],
            bids: bids.clone(),
            asks: asks.clone(),
            tick: self.tick,
            spacing: self.ticks.tick_spacing as u64,
        }
    }
}

impl ToLiquidityBook for SrzUniswapV4State {
    fn structurate(&self, component: SrzProtocolComponent, tks: Vec<SrzToken>, query: PairQuery, fee: f64, price: f64, poolb0: f64, poolb1: f64) -> LiquidityPoolBook {
        LiquidityPoolBook {
            address: component.id.clone().to_lowercase(),
            protocol: component.protocol_type_name.clone(),
            concentrated: true,
            fee,
            price,
            reserves: vec![poolb0, poolb1],
            bids: vec![], //bids.clone(),
            asks: vec![], //asks.clone(),
            tick: self.tick,
            spacing: self.ticks.tick_spacing as u64,
        }
    }
}

// =====================================================================================================================================================================================================
// ========================================================================================= LIQUIDITY ================================================================================================
// =====================================================================================================================================================================================================

/// @notice Reading 'state' from Redis DB while using TychoStreamState state and functions to compute/simulate might create a inconsistency
/// @notice This function is used to compute the liquidity on multiple pools from differents AMM, and return a unified liquidity structure
pub async fn build(network: Network, datapools: Vec<ProtoTychoState>, tokens: Vec<SrzToken>, query: PairQuery) -> PairLiquidityBook {
    log::info!("Got {} pools to compute for pair: '{}'", datapools.len(), query.tag);
    let mut orderbooks = Vec::new();
    for pdata in datapools.clone() {
        log::info!("Preparing pool: {} | Type: {}", pdata.component.id, pdata.component.protocol_type_name);
        let srzt0 = tokens[0].clone();
        let srzt1 = tokens[1].clone();
        let srztokens = vec![srzt0.clone(), srzt1.clone()];
        // if pdata.component.id.to_lowercase() != "0x8ad599c3a0ff1de082011efddc58f1908eb6e6d8" {
        //     log::info!("Skipping pool: {}", pdata.component.id);
        //     continue;
        // }
        if pdata.component.protocol_type_name.to_lowercase() == "uniswap_v4_pool" || pdata.component.protocol_type_name.to_lowercase() == "balancer_v2_pool" {
            log::info!("Skipping pool {} because it's {}", pdata.component.id, pdata.component.protocol_type_name.to_lowercase());
            continue;
        }
        let t0 = Token::from(srzt0.clone());
        let t1 = Token::from(srzt1.clone());
        let provider = ProviderBuilder::new().with_chain(alloy_chains::NamedChain::Mainnet).on_http(network.rpc.clone().parse().unwrap());
        let cpbs = get_pool_balances(network.clone(), &provider, pdata.component.clone()).await;
        let (poolb0, poolb1) = (cpbs[0], cpbs[1]);
        log::info!("Pool cpbs: {}-{} => {} and {}", t0.symbol, t1.symbol, poolb0, poolb1);
        // let (base, quote) = if query.z0to1 { (t0, t1) } else { (t1, t0) };
        let (base, quote) = (t0, t1); // !
        let proto = pdata.protosim.clone();
        let price = proto.spot_price(&base, &quote).unwrap_or_default();
        log::info!("Spot price for {}-{} => {}", base.symbol, quote.symbol, price);
        match AmmType::from(pdata.component.protocol_type_name.clone().as_str()) {
            AmmType::Pancakeswap | AmmType::Sushiswap | AmmType::UniswapV2 => {
                if let Some(state) = proto.as_any().downcast_ref::<UniswapV2State>() {
                    let state = SrzUniswapV2State::from((state.clone(), pdata.component.id.clone()));
                    let fee = proto.fee();
                    let book = state.clone().structurate(pdata.component.clone(), srztokens.clone(), query.clone(), fee, price, poolb0, poolb1);
                    orderbooks.push(book);
                }
            }
            AmmType::UniswapV3 => {
                if let Some(state) = proto.as_any().downcast_ref::<UniswapV3State>() {
                    let state = SrzUniswapV3State::from((state.clone(), pdata.component.id.clone()));
                    let fee = proto.fee();
                    let book = state.clone().structurate(pdata.component.clone(), srztokens.clone(), query.clone(), fee, price, poolb0, poolb1);
                    orderbooks.push(book);
                }
            }
            AmmType::UniswapV4 => {
                if let Some(state) = proto.as_any().downcast_ref::<UniswapV4State>() {
                    let state = SrzUniswapV4State::from((state.clone(), pdata.component.id.clone()));
                    let fee = proto.fee();
                    let book = state.clone().structurate(pdata.component.clone(), srztokens.clone(), query.clone(), fee, price, poolb0, poolb1);
                    orderbooks.push(book);
                }
            }
            AmmType::Curve | AmmType::Balancer => {
                if let Some(state) = proto.as_any().downcast_ref::<EVMPoolState<PreCachedDB>>() {
                    let state = SrzEVMPoolState::from((state.clone(), pdata.component.id.to_string()));
                    let fee = 0.0; // proto.fee(); // Tycho does not have a fee function implemented for EVMPoolState
                    let book = state.clone().structurate(pdata.component.clone(), srztokens.clone(), query.clone(), fee, price, poolb0, poolb1);
                    orderbooks.push(book);
                }
            }
        }
        log::info!("\n");
    }

    // let path = format!("misc/data/{}.eth-usdc.pair-orderbook.json", network.name);
    // crate::shd::utils::misc::save1(output.clone(), path.as_str());
    PairLiquidityBook {
        from: tokens[0].clone(),
        to: tokens[1].clone(),
        orderbooks: orderbooks.clone(),
    }
}
