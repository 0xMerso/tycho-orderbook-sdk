use tycho_simulation::{models::Token, protocol::state::ProtocolSim};

use crate::shd::{
    self,
    data::fmt::{SrzProtocolComponent, SrzToken},
    r#static::maths::{simu, TEN_MILLIONS},
    types::{MidPriceData, Network, Orderbook, OrderbookFunctions, OrderbookRequestParams, ProtoTychoState, TradeResult},
};
use std::{collections::HashMap, time::Instant};

/// @notice Reading 'state' from Redis DB while using TychoStreamState state and functions to compute/simulate might create a inconsistency
pub async fn build(network: Network, ptss: Vec<ProtoTychoState>, tokens: Vec<SrzToken>, query: OrderbookRequestParams, simufns: Option<OrderbookFunctions>, t0_worth_eth: f64, t1_worth_eth: f64) -> Orderbook {
    log::info!("Building orderbook ... Got {} pools to compute for pair: '{}'", ptss.len(), query.tag);
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
    let mut balances = HashMap::new();
    for pdata in ptss.clone() {
        pools.push(pdata.clone());
        let proto = pdata.protosim.clone();
        let price0to1 = proto.spot_price(&base, &quote).unwrap_or_default();
        let price1to0 = proto.spot_price(&quote, &base).unwrap_or_default();
        prices0to1.push(price0to1);
        prices1to0.push(price1to0);
        log::info!(
            "- Pool: {} | {} | Spot price for {}-{} => price0to1 = {} and price1to0 = {}",
            pdata.component.id,
            pdata.component.protocol_type_name,
            base.symbol,
            quote.symbol,
            price0to1,
            price1to0
        );
        if let Some(cpbs) = shd::core::client::cpbs(network.clone(), pdata.component.id.clone(), pdata.component.protocol_system.clone()).await {
            let t0b = cpbs.get(&srzt0.address.to_lowercase()).unwrap_or(&0u128);
            let t0b = *t0b as f64 / 10f64.powi(srzt0.decimals as i32);
            aggt0lqdty.push(t0b);
            let t1b = cpbs.get(&srzt1.address.to_lowercase()).unwrap_or(&0u128);
            let t1b = *t1b as f64 / 10f64.powi(srzt1.decimals as i32);
            aggt1lqdty.push(t1b);
            let mut tmpb = HashMap::new();
            tmpb.insert(srzt0.address.clone(), t0b);
            tmpb.insert(srzt1.address.clone(), t1b);
            balances.insert(pdata.component.id.clone().to_lowercase(), tmpb);
        }
    }
    let cps: Vec<SrzProtocolComponent> = pools.clone().iter().map(|p| p.component.clone()).collect();
    let aggregated = shd::maths::steps::deepth(cps.clone(), tokens.clone(), balances.clone());
    let avgp0to1 = prices0to1.iter().sum::<f64>() / prices0to1.len() as f64;
    let avgp1to0 = prices1to0.iter().sum::<f64>() / prices1to0.len() as f64; // Ponderation by TVL ?
    log::info!("Average price 0to1: {} | Average price 1to0: {}", avgp0to1, avgp1to0);
    let mut pso = simulate(network.clone(), pools.clone(), tokens, query.clone(), simufns, aggregated.clone(), t0_worth_eth, t1_worth_eth).await;
    pso.prices0to1 = prices0to1.clone();
    pso.prices1to0 = prices1to0.clone();
    pso.aggt0lqdty = aggt0lqdty.clone();
    pso.aggt1lqdty = aggt1lqdty.clone();
    log::info!("Optimization done. Returning Simulated Orderbook for pair (base-quote) => '{}-{}'\n", base.symbol, quote.symbol);
    pso
}

/**
 * Optimizes a trade for a given pair of tokens and a set of pools.
 * The function generates a set of test amounts for ETH and USDC, then runs the optimizer for each amount.
 * The optimizer uses a simple gradient-based approach to move a fixed fraction of the allocation from the pool with the lowest marginal return to the one with the highest.
 * If the query specifies a specific token to sell with a specific amount, the optimizer will only run for that token and amount.
 */
pub async fn simulate(
    network: Network,
    pcsdata: Vec<ProtoTychoState>,
    tokens: Vec<SrzToken>,
    body: OrderbookRequestParams,
    simufns: Option<OrderbookFunctions>,
    balances: HashMap<String, f64>,
    t0_worth_eth: f64,
    t1_worth_eth: f64,
) -> Orderbook {
    let eth_usd = shd::core::gas::eth_usd().await;
    let gas_price = shd::core::gas::gas_price(network.rpc).await;
    let t0 = tokens[0].clone();
    let t1 = tokens[1].clone();
    let aggbt0 = balances.iter().find(|x| x.0.to_lowercase() == t0.address.to_lowercase()).unwrap().1;
    let aggbt1 = balances.iter().find(|x| x.0.to_lowercase() == t1.address.to_lowercase()).unwrap().1;

    log::info!(
        "🔎 Optimisation | Network: {} | ETH is worth {} in USD | Got {} pools to optimize for pair: {}-{} with aggbs {:.4} and {:.4}",
        network.name,
        eth_usd,
        pcsdata.len(),
        t0.symbol,
        t1.symbol,
        aggbt0,
        aggbt1
    );
    let pools = pcsdata.iter().map(|x| x.component.clone()).collect::<Vec<SrzProtocolComponent>>();

    // Best bid/ask. Need to remove gas consideration here ? I don't think so
    let amount_eth = 1. / 1000.; // 1/100 of ETH = ~2$ (for 2000$ ETH)
    let amount_test_best0to1 = amount_eth / t0_worth_eth;
    let amount_test_best1to0 = amount_eth / t1_worth_eth;
    let best0to1 = best(&pcsdata, eth_usd, gas_price, &t0, &t1, amount_test_best0to1, t1_worth_eth);
    let best1to0 = best(&pcsdata, eth_usd, gas_price, &t1, &t0, amount_test_best1to0, t0_worth_eth);
    let mpd0to1 = midprice(best0to1.clone(), best1to0.clone());
    let mpd1to0 = midprice(best1to0.clone(), best0to1.clone());

    let mut result = Orderbook {
        token0: tokens[0].clone(),
        token1: tokens[1].clone(),
        pools: pools.clone(),
        trades0to1: vec![], // Set depending query params
        trades1to0: vec![], // Set depending query params
        prices0to1: vec![], // Set later
        prices1to0: vec![], // Set later
        aggt0lqdty: vec![], // Set later
        aggt1lqdty: vec![], // Set later
        eth_usd,
        // best0to1: best0to1.clone(),
        // best1to0: best1to0.clone(),
        mpd0to1: mpd0to1.clone(),
        mpd1to0: mpd1to0.clone(),
        t0_worth_eth,
        t1_worth_eth,
    };
    match body.sps {
        Some(spsq) => {
            log::info!(" 🎯 Partial Optimisation: input: {} and amount: {}", spsq.input, spsq.amount);
            if spsq.input.to_lowercase() == t0.address.to_lowercase() {
                result.trades0to1 = vec![shd::maths::opti::gradient(spsq.amount, &pcsdata, t0.clone(), t1.clone(), eth_usd, gas_price, t1_worth_eth)];
            } else if spsq.input.to_lowercase() == t1.address.to_lowercase() {
                result.trades1to0 = vec![shd::maths::opti::gradient(spsq.amount, &pcsdata, t1.clone(), t0.clone(), eth_usd, gas_price, t0_worth_eth)];
            }
        }
        None => {
            let fn_opti = match simufns {
                Some(fns) => fns.optimize,
                None => optimize,
            };
            // FuLL Orderbook optimization
            let trades0to1 = (fn_opti)(&pcsdata, eth_usd, gas_price, &t0, &t1, *aggbt0, t1_worth_eth);
            result.trades0to1 = trades0to1;
            log::info!(" 🔄  Switching to 1to0");
            let trades1to0 = (fn_opti)(&pcsdata, eth_usd, gas_price, &t1, &t0, *aggbt1, t0_worth_eth);
            result.trades1to0 = trades1to0;
        }
    }
    result
}

pub type OrderbookQuoteFn = fn(pcs: &Vec<ProtoTychoState>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, aggb: f64, output_u_ethworth: f64) -> Vec<TradeResult>;

pub fn optimize_fast(pcs: &Vec<ProtoTychoState>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, aggb: f64, output_u_ethworth: f64) -> Vec<TradeResult> {
    let mut trades = Vec::new();
    let start = aggb / TEN_MILLIONS; // No longer needed: / 10f64.powi(from.decimals as i32);
    log::info!("Agg onchain liquidity balance for {} is {} (for 1 millionth => {}) | Output unit worth eth: {}", from.symbol, aggb, start, output_u_ethworth);
    let steps = shd::maths::steps::exponential(
        shd::r#static::maths::simu::COUNT_FAST,
        shd::r#static::maths::simu::START_MULTIPLIER,
        shd::r#static::maths::simu::END_MULTIPLIER,
        shd::r#static::maths::simu::END_MULTIPLIER * shd::r#static::maths::simu::MIN_EXP_DELTA_PCT,
    );
    let steps = steps.iter().map(|x| x * start).collect::<Vec<f64>>();
    for (x, amount) in steps.iter().enumerate() {
        let tmstp = Instant::now();
        let result = shd::maths::opti::gradient(*amount, pcs, from.clone(), to.clone(), eth_usd, gas_price, output_u_ethworth);
        let elapsed = tmstp.elapsed().as_millis();
        let gas_cost = result.gas_costs_usd.iter().sum::<f64>();
        log::info!(
            " - #{:<2} | In: {:.7} {}, Out: {:.7} {} at price {} | Gas cost {:.5}$ | Distribution: {:?} | Took: {} ms",
            x,
            result.amount,
            from.symbol,
            result.output,
            to.symbol,
            result.average_sell_price,
            gas_cost,
            result.distribution,
            elapsed
        );
        trades.push(result);
    }
    trades
}

use rayon::prelude::*; // Ensure Rayon is in your dependencies.

/**
 * Executes the optimizer for a given token pair and a set of pools.
 */
pub fn optimize(pcs: &Vec<ProtoTychoState>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, aggb: f64, output_u_ethworth: f64) -> Vec<TradeResult> {
    let start = aggb / TEN_MILLIONS;
    log::info!("Agg onchain liquidity balance for {} is {} (for 1 millionth => {}) | Output unit worth eth: {}", from.symbol, aggb, start, output_u_ethworth);
    let steps = shd::maths::steps::exponential(simu::COUNT, simu::START_MULTIPLIER, simu::END_MULTIPLIER, simu::END_MULTIPLIER * simu::MIN_EXP_DELTA_PCT);
    let steps = steps.iter().map(|x| x * start).collect::<Vec<f64>>();
    let trades: Vec<TradeResult> = steps
        .par_iter()
        .enumerate()
        .map(|(x, amount)| {
            let tmstp = Instant::now();
            let result = shd::maths::opti::gradient(*amount, pcs, from.clone(), to.clone(), eth_usd, gas_price, output_u_ethworth);
            let elapsed = tmstp.elapsed().as_millis();
            let gas_cost = result.gas_costs_usd.iter().sum::<f64>();
            log::info!(
                " - #{:<2} | In: {:.7} {}, Out: {:.7} {} at price {} | Gas cost {:.5}$ | Distribution: {:?} | Took: {} ms",
                x,
                result.amount,
                from.symbol,
                result.output,
                to.symbol,
                result.average_sell_price,
                gas_cost,
                result.distribution,
                elapsed
            );
            result
        })
        .collect();
    trades
}

/**
 * Computes the mid price for a given token pair
 * We cannot replicate the logic of a classic orderbook as we don't have best bid/ask exacly
 * In theory it would be : Mid Price = (Best Bid Price + Best Ask Price) / 2
 * Applied to AMM, we choose to use a small amountIn = 1 / TEN_MILLIONS of the aggregated liquidity
 * Doing that for 0to1 and 1to0 we have our best bid/ask, then we can compute the mid price
 * --- --- --- --- ---
 * Amount out is net of gas cost
 */
pub fn best(pcs: &Vec<ProtoTychoState>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, amount: f64, output_u_ethworth: f64) -> TradeResult {
    log::info!(" - 🥇 Computing best price for {} (amount in = {})", from.symbol, amount);
    let result = shd::maths::opti::gradient(amount, pcs, from.clone(), to.clone(), eth_usd, gas_price, output_u_ethworth);
    log::info!(
        " - (best) Input: {} {}, Output: {} {} at price {} | Distribution: {:?} ",
        result.amount,
        from.symbol,
        result.output,
        to.symbol,
        result.average_sell_price,
        result.distribution
    );
    result
}

/**
 * Computes the mid price for a given token pair using the best bid and ask
 * ! We assume that => trade0t1 = ask and trade1to0 = bid
 */
pub fn midprice(trade0t1: TradeResult, trade1to0: TradeResult) -> MidPriceData {
    let best_ask = trade0t1.average_sell_price;
    let best_bid = 1. / trade1to0.average_sell_price;
    let mid = (best_ask + best_bid) / 2.;
    let spread = (best_ask - best_bid).abs();
    let spread_pct = (spread / mid) * 100.;
    // log::info!(" - midprice: best_ask: {}", best_ask);
    // log::info!(" - midprice: best_bid: {}", best_bid);
    // log::info!(" - midprice: trade1to0.ratio: {}", trade1to0.ratio);
    // log::info!(" - midprice: mid: {}", mid);
    // log::info!(" - midprice: spread: {}", spread);
    // log::info!(" - midprice: spread_pct: {}", spread_pct);
    MidPriceData { best_ask, best_bid, mid, spread, spread_pct }
}

/// Check if a component has the desired tokens
pub fn matchcp(cptks: Vec<SrzToken>, tokens: Vec<SrzToken>) -> bool {
    tokens.iter().all(|token| cptks.iter().any(|cptk| cptk.address.eq_ignore_ascii_case(&token.address)))
}
