use tycho_simulation::{models::Token, protocol::state::ProtocolSim};

use crate::shd::{
    self,
    data::fmt::{SrzProtocolComponent, SrzToken},
    r#static::maths::TEN_MILLIONTH,
    types::{MidPriceData, Network, OrderbookRequestBody, PairSimulatedOrderbook, ProtoTychoState, TradeResult},
};
use std::{collections::HashMap, time::Instant};

/// @notice Reading 'state' from Redis DB while using TychoStreamState state and functions to compute/simulate might create a inconsistency
pub async fn build(network: Network, balances: HashMap<String, HashMap<String, u128>>, ptss: Vec<ProtoTychoState>, tokens: Vec<SrzToken>, query: OrderbookRequestBody, utk0_ethworth: f64, utk1_ethworth: f64) -> PairSimulatedOrderbook {
    log::info!("Got {} pools to compute for pair: '{}'", ptss.len(), query.tag);
    let mut pools = Vec::new();
    let mut prices0to1 = vec![];
    let mut prices1to0 = vec![];
    let srzt0 = tokens[0].clone();
    let srzt1 = tokens[1].clone();
    let t0 = Token::from(srzt0.clone());
    let t1 = Token::from(srzt1.clone());
    let (base, quote) = (t0, t1);
    let mut aggt0lqdty = vec![];
    let mut aggt1lqdty = vec![];
    for pdata in ptss.clone() {
        pools.push(pdata.clone());
        let proto = pdata.protosim.clone();
        let price0to1 = proto.spot_price(&base, &quote).unwrap_or_default();
        let price1to0 = proto.spot_price(&quote, &base).unwrap_or_default();
        prices0to1.push(price0to1);
        prices1to0.push(price1to0);
        log::info!(
            "- Preparing pool: {} | Type: {} | Spot price for {}-{} => price0to1 = {} and price1to0 = {}",
            pdata.component.id,
            pdata.component.protocol_type_name,
            base.symbol,
            quote.symbol,
            price0to1,
            price1to0
        );
        if let Some(cpbs) = balances.get(&pdata.component.id.to_lowercase()) {
            let t0b = cpbs.get(&srzt0.address.to_lowercase()).unwrap_or(&0u128);
            aggt0lqdty.push(*t0b as f64 / 10f64.powi(srzt0.decimals as i32));
            let t1b = cpbs.get(&srzt1.address.to_lowercase()).unwrap_or(&0u128);
            aggt1lqdty.push(*t1b as f64 / 10f64.powi(srzt1.decimals as i32));
        }
    }
    let cps: Vec<SrzProtocolComponent> = pools.clone().iter().map(|p| p.component.clone()).collect();
    let aggregated = shd::maths::steps::deepth(cps.clone(), tokens.clone(), balances.clone());
    let avgp0to1 = prices0to1.iter().sum::<f64>() / prices0to1.len() as f64;
    let avgp1to0 = prices1to0.iter().sum::<f64>() / prices1to0.len() as f64; // Ponderation by TVL ?
    log::info!("Average price 0to1: {} | Average price 1to0: {}", avgp0to1, avgp1to0);

    // return PairSimulatedOrderbook {
    //     token0: srzt0.clone(),
    //     token1: srzt1.clone(),
    //     trades0to1: vec![],
    //     trades1to0: vec![],
    //     prices0to1: prices0to1.clone(),
    //     prices1to0: prices1to0.clone(),
    //     pools: vec![],
    // };

    let mut pso = simulate(network.clone(), pools.clone(), tokens, query.clone(), aggregated.clone(), utk0_ethworth, utk1_ethworth).await;
    pso.prices0to1 = prices0to1.clone();
    pso.prices1to0 = prices1to0.clone();
    pso.aggt0lqdty = aggt0lqdty.clone();
    pso.aggt1lqdty = aggt1lqdty.clone();
    log::info!("Optimization done. Returning Simulated Orderbook for pair (base-quote) => '{}-{}'", base.symbol, quote.symbol);
    pso
}

/**
 * Optimizes a trade for a given pair of tokens and a set of pools.
 * The function generates a set of test amounts for ETH and USDC, then runs the optimizer for each amount.
 * The optimizer uses a simple gradient-based approach to move a fixed fraction of the allocation from the pool with the lowest marginal return to the one with the highest.
 * If the query specifies a specific token to sell with a specific amount, the optimizer will only run for that token and amount.
 */
pub async fn simulate(network: Network, pcsdata: Vec<ProtoTychoState>, tokens: Vec<SrzToken>, body: OrderbookRequestBody, balances: HashMap<String, u128>, utk0_ethworth: f64, utk1_ethworth: f64) -> PairSimulatedOrderbook {
    let eth_usd = shd::core::gas::eth_usd().await;
    let gas_price = shd::core::gas::gas_price(network.rpc).await;
    let t0 = tokens[0].clone();
    let t1 = tokens[1].clone();
    let aggbt0 = balances.iter().find(|x| x.0.to_lowercase() == t0.address.to_lowercase()).unwrap().1;
    let aggbt1 = balances.iter().find(|x| x.0.to_lowercase() == t1.address.to_lowercase()).unwrap().1;

    log::info!(
        "🔎 Optimisation | Network: {} | ETH is worth {} in USD | Got {} pools to optimize for pair: {}-{}",
        network.name,
        eth_usd,
        pcsdata.len(),
        t0.symbol,
        t1.symbol
    );
    let pools = pcsdata.iter().map(|x| x.component.clone()).collect::<Vec<SrzProtocolComponent>>();

    // Best bid/ask
    let best0to1 = best(&pcsdata, eth_usd, gas_price, &t0, &t1, aggbt0.clone(), utk1_ethworth);
    let best1to0 = best(&pcsdata, eth_usd, gas_price, &t1, &t0, aggbt1.clone(), utk0_ethworth);

    let mpd0to1 = mid_price_data(best0to1.clone(), best1to0.clone());
    let mpd1to0 = mid_price_data(best1to0.clone(), best0to1.clone());

    let mut result = PairSimulatedOrderbook {
        token0: tokens[0].clone(),
        token1: tokens[1].clone(),
        pools: pools.clone(),
        trades0to1: vec![], // Set depending query params
        trades1to0: vec![], // Set depending query params
        prices0to1: vec![], // Set later
        prices1to0: vec![], // Set later
        aggt0lqdty: vec![], // Set later
        aggt1lqdty: vec![], // Set later
        eth_usd: eth_usd.clone(),
        // best0to1: best0to1.clone(),
        // best1to0: best1to0.clone(),
        mpd0to1: mpd0to1.clone(),
        mpd1to0: mpd1to0.clone(),
        t0_worth_eth: utk0_ethworth.clone(),
        t1_worth_eth: utk1_ethworth.clone(),
    };
    match body.spsq {
        Some(spsq) => {
            log::info!("🎯 Partial Optimisation: input: {} and amount: {}", spsq.input, spsq.amount);
            if spsq.input.to_lowercase() == t0.address.to_lowercase() {
                let power = 10f64.powi(t0.decimals as i32);
                let amount = (spsq.amount * power).floor();
                // result.trades0to1 = optimize(&balances, &pcsdata, eth_usd, gas_price, &t0, &t1, utk1_ethworth);
                result.trades0to1 = vec![shd::maths::opti::gradient(amount, &pcsdata, t0.clone(), t1.clone(), eth_usd, gas_price, utk1_ethworth)];
                dbg!(result.trades0to1.clone());
            } else if spsq.input.to_lowercase() == t1.address.to_lowercase() {
                let power = 10f64.powi(t1.decimals as i32);
                let amount = (spsq.amount * power).floor();
                // result.trades1to0 = optimize(&balances, &pcsdata, eth_usd, gas_price, &t1, &t0, utk0_ethworth);
                result.trades1to0 = vec![shd::maths::opti::gradient(amount, &pcsdata, t1.clone(), t0.clone(), eth_usd, gas_price, utk0_ethworth)];
                dbg!(result.trades1to0.clone());
            }
        }
        None => {
            // FuLL Orderbook optimization
            let trades0to1 = optimize(&pcsdata, eth_usd, gas_price, &t0, &t1, aggbt0.clone(), utk1_ethworth);
            result.trades0to1 = trades0to1;
            log::info!(" 🔄  Switching to 1to0");
            let trades1to0 = optimize(&pcsdata, eth_usd, gas_price, &t1, &t0, aggbt1.clone(), utk0_ethworth);
            result.trades1to0 = trades1to0;
        }
    }

    result
}

/**
 * Executes the optimizer for a given token pair and a set of pools.
 */
pub fn optimize(pcs: &Vec<ProtoTychoState>, ethusd: f64, gasp: u128, from: &SrzToken, to: &SrzToken, aggb: u128, output_u_ethworth: f64) -> Vec<TradeResult> {
    let mut trades = Vec::new();
    let start = aggb as f64 / TEN_MILLIONTH / 10f64.powi(from.decimals as i32);
    log::info!("Agg onchain liquidity balance for {} is {} (for 1 millionth => {}) | Output unit worth: {}", from.symbol, aggb, start, output_u_ethworth);
    let steps = shd::maths::steps::exponential(
        shd::r#static::maths::simu::COUNT,
        shd::r#static::maths::simu::START_MULTIPLIER,
        shd::r#static::maths::simu::END_MULTIPLIER,
        shd::r#static::maths::simu::MIN_EXP_DELTA,
    );
    let steps = steps.iter().map(|x| x * start).collect::<Vec<f64>>();
    for (x, amount) in steps.iter().enumerate() {
        let start = Instant::now();
        let result = shd::maths::opti::gradient(*amount, pcs, from.clone(), to.clone(), ethusd, gasp, output_u_ethworth);
        let elapsed = start.elapsed();
        log::info!(
            " - #{x} | Input: {} {}, Output: {} {} at price {} | Distribution: {:?} | Took: {:?}",
            result.amount,
            from.symbol,
            result.output,
            to.symbol,
            result.ratio,
            result.distribution,
            elapsed
        );
        trades.push(result);
    }
    trades
}

/**
 * Computes the mid price for a given token pair
 * We cannot replicate the logic of a classic orderbook as we don't have best bid/ask exacly
 * In theory it would be : Mid Price = (Best Bid Price + Best Ask Price) / 2
 * Applied to AMM, we choose to use a small amountIn = 1 / TEN_MILLIONTH of the aggregated liquidity
 * Doing that for 0to1 and 1to0 we have our best bid/ask, then we can compute the mid price
 * --- --- --- --- ---
 * Amount out is net of gas cost
 */
pub fn best(pcs: &Vec<ProtoTychoState>, ethusd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, aggb: u128, output_u_ethworth: f64) -> TradeResult {
    let amount = aggb as f64 / TEN_MILLIONTH / 10f64.powi(from.decimals as i32);
    log::info!(" - 🥇 Computing best price for {} (amount in = {})", from.symbol, amount);
    let result = shd::maths::opti::gradient(amount, pcs, from.clone(), to.clone(), ethusd, gas_price, output_u_ethworth);
    log::info!(
        " - (best) Input: {} {}, Output: {} {} at price {} | Distribution: {:?} ",
        result.amount,
        from.symbol,
        result.output,
        to.symbol,
        result.ratio,
        result.distribution
    );
    result
}

/**
 * Computes the mid price for a given token pair using the best bid and ask
 * ! We assume that => trade0t1 = ask and trade1to0 = bid
 */
pub fn mid_price_data(trade0t1: TradeResult, trade1to0: TradeResult) -> MidPriceData {
    let best_ask = trade0t1.ratio;
    // log::info!(" - mid_price_data: best_ask: {}", best_ask);
    let best_bid = 1. / trade1to0.ratio;
    // log::info!(" - mid_price_data: best_bid: {}", best_bid);
    let mid = (best_ask + best_bid) / 2.;
    log::info!(" - mid_price_data: mid: {}", mid);
    let spread = (best_ask - best_bid).abs();
    log::info!(" - mid_price_data: spread: {}", spread);
    let spread_pct = (spread / mid) * 100.;
    log::info!(" - mid_price_data: spread_pct: {}", spread_pct);
    let _inverse_price0t1 = 1. / best_ask;
    // log::info!(" - mid_price_data: inverse_price0t1: {}", inverse_price0t1);
    let _inverse_price1t0 = 1. / trade1to0.ratio;
    // log::info!(" - mid_price_data: inverse_price1t0: {}", inverse_price1t0);
    MidPriceData {
        best_ask,
        best_bid,
        mid,
        spread,
        spread_pct,
        // inverse_price0t1,
        // inverse_price1t0,
    }
}
