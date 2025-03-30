use tycho_simulation::models::Token;

use crate::shd::{
    self,
    data::fmt::{SrzProtocolComponent, SrzToken},
    r#static::maths::{simu, BEST_BID_ASK_ETH_BPS, BPD, TEN_MILLIONS},
    types::{MidPriceData, Network, Orderbook, OrderbookFunctions, OrderbookRequestParams, ProtoTychoState, TradeResult},
};
use rayon::prelude::*;
use std::{collections::HashMap, time::Instant}; // Ensure Rayon is in your dependencies.

/// @notice Reading 'state' from Redis DB while using TychoStreamState state and functions to compute/simulate might create a inconsistency
pub async fn build(
    network: Network,
    api_token: Option<String>,
    ptss: Vec<ProtoTychoState>,
    tokens: Vec<SrzToken>,
    query: OrderbookRequestParams,
    simufns: Option<OrderbookFunctions>,
    base_worth_eth: f64,
    quote_worth_eth: f64,
) -> Orderbook {
    log::info!("Building orderbook ... Got {} pools to compute for pair: '{}'", ptss.len(), query.tag);
    let mut pools = Vec::new();
    let mut prices_base_to_quote = vec![];
    let mut prices_quote_to_base = vec![];
    let srzt0 = tokens[0].clone();
    let srzt1 = tokens[1].clone();
    let t0 = Token::from(srzt0.clone());
    let t1 = Token::from(srzt1.clone());
    // ! Assume that the first token is the base and the second is the quote, so bid = 'buy base', and ask = 'sell base'. It's the responsibility of the caller to ensure this.
    let (base, quote) = (t0, t1);
    let mut base_lqdty = vec![];
    let mut quote_lqdty = vec![];
    let mut balances = HashMap::new();
    for pdata in ptss.clone() {
        pools.push(pdata.clone());
        let proto = pdata.protosim.clone();
        let price_base_to_quote = proto.spot_price(&base, &quote).unwrap_or_default();
        let price_quote_to_base = proto.spot_price(&quote, &base).unwrap_or_default();
        prices_base_to_quote.push(price_base_to_quote);
        prices_quote_to_base.push(price_quote_to_base);
        log::info!(
            "- Pool: {} | {} | Spot price for {}-{} => price_base_to_quote = {} and price_quote_to_base = {}",
            pdata.component.id,
            pdata.component.protocol_type_name,
            base.symbol,
            quote.symbol,
            price_base_to_quote,
            price_quote_to_base
        );
        if let Some(cpbs) = shd::core::rpc::get_component_balances(network.clone(), pdata.component.id.clone(), pdata.component.protocol_system.clone(), api_token.clone()).await {
            let base_bal = cpbs.get(&srzt0.address.to_lowercase()).unwrap_or(&0u128);
            let base_bal = *base_bal as f64 / 10f64.powi(srzt0.decimals as i32);
            base_lqdty.push(base_bal);
            let quote_bal = cpbs.get(&srzt1.address.to_lowercase()).unwrap_or(&0u128);
            let quote_bal = *quote_bal as f64 / 10f64.powi(srzt1.decimals as i32);
            quote_lqdty.push(quote_bal);
            let mut tmpb = HashMap::new();
            tmpb.insert(srzt0.address.clone(), base_bal);
            tmpb.insert(srzt1.address.clone(), quote_bal);
            balances.insert(pdata.component.id.clone().to_lowercase(), tmpb);
        }
    }
    let cps: Vec<SrzProtocolComponent> = pools.clone().iter().map(|p| p.component.clone()).collect();
    let aggregated = shd::maths::steps::depth(cps.clone(), tokens.clone(), balances.clone());
    let avg_price_base_to_quote = prices_base_to_quote.iter().sum::<f64>() / prices_base_to_quote.len() as f64;
    let avg_price_quote_to_base = prices_quote_to_base.iter().sum::<f64>() / prices_quote_to_base.len() as f64; // Ponderation by TVL ?
    log::info!("Average price 0to1: {} | Average price 1to0: {}", avg_price_base_to_quote, avg_price_quote_to_base);
    let mut pso = simulate(network.clone(), pools.clone(), tokens, query.clone(), simufns, aggregated.clone(), base_worth_eth, quote_worth_eth).await;
    pso.prices_base_to_quote = prices_base_to_quote;
    pso.prices_quote_to_base = prices_quote_to_base;
    pso.base_lqdty = base_lqdty.clone();
    pso.quote_lqdty = quote_lqdty.clone();
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
    base_worth_eth: f64,
    quote_worth_eth: f64,
) -> Orderbook {
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("Time went backwards").as_secs();
    let eth_usd = shd::core::gas::eth_usd().await;
    let gas_price = shd::core::gas::gas_price(network.rpc.clone()).await;
    let latest = shd::core::gas::get_latest_block(network.rpc.clone()).await;
    let base = tokens[0].clone();
    let quote = tokens[1].clone();
    let aggb_base = balances.iter().find(|x| x.0.to_lowercase() == base.address.to_lowercase()).unwrap().1;
    let aggb_quote = balances.iter().find(|x| x.0.to_lowercase() == quote.address.to_lowercase()).unwrap().1;

    log::info!(
        "🔎 Optimisation | Network: {} | ETH is worth {} in USD | Got {} pools to optimize for pair: {}-{} with aggbs {:.4} and {:.4}",
        network.name,
        eth_usd,
        pcsdata.len(),
        base.symbol,
        quote.symbol,
        aggb_base,
        aggb_quote
    );

    let pools = pcsdata.iter().map(|x| x.component.clone()).collect::<Vec<SrzProtocolComponent>>();
    let amount_eth = BEST_BID_ASK_ETH_BPS / BPD; // 1/100 of ETH = ~2$ (for 2000$ ETH)
    let amount_test_best_base_to_quote = amount_eth / base_worth_eth;
    let amount_test_best_quote_to_base = amount_eth / quote_worth_eth;
    let best_base_to_quote = best(&pcsdata, eth_usd, gas_price, &base, &quote, amount_test_best_base_to_quote, quote_worth_eth);
    let best_quote_to_base = best(&pcsdata, eth_usd, gas_price, &quote, &base, amount_test_best_quote_to_base, base_worth_eth);
    let mpd_base_to_quote = midprice(best_base_to_quote.clone(), best_quote_to_base.clone());
    let mpd_quote_to_base = midprice(best_quote_to_base.clone(), best_base_to_quote.clone());

    let mut result = Orderbook {
        block: latest,
        timestamp,
        base: tokens[0].clone(),
        quote: tokens[1].clone(),
        pools: pools.clone(),
        bids: vec![],                 // Set depending query params
        asks: vec![],                 // Set depending query params
        prices_base_to_quote: vec![], // Set later
        prices_quote_to_base: vec![], // Set later
        base_lqdty: vec![],           // Set later
        quote_lqdty: vec![],          // Set later
        eth_usd,
        mpd_base_to_quote: mpd_base_to_quote.clone(),
        mpd_quote_to_base: mpd_quote_to_base.clone(),
        base_worth_eth,
        quote_worth_eth,
    };
    match body.sps {
        Some(spsq) => {
            log::info!(" 🎯 Partial Optimisation: input: {} and amount: {}", spsq.input, spsq.amount);
            if spsq.input.to_lowercase() == base.address.to_lowercase() {
                result.bids = vec![shd::maths::opti::gradient(spsq.amount, &pcsdata, base.clone(), quote.clone(), eth_usd, gas_price, quote_worth_eth)];
            } else if spsq.input.to_lowercase() == quote.address.to_lowercase() {
                result.asks = vec![shd::maths::opti::gradient(spsq.amount, &pcsdata, quote.clone(), base.clone(), eth_usd, gas_price, base_worth_eth)];
            }
        }
        None => {
            let fn_opti = match simufns {
                Some(fns) => fns.optimize,
                None => optimize,
            };
            // FuLL Orderbook optimization
            let bids = (fn_opti)(&pcsdata, eth_usd, gas_price, &base, &quote, *aggb_base, quote_worth_eth);
            result.bids = bids;
            log::info!(" 🔄  Switching to 1to0");
            let asks = (fn_opti)(&pcsdata, eth_usd, gas_price, &quote, &base, *aggb_quote, base_worth_eth);
            result.asks = asks;
        }
    }
    result
}

pub type OrderbookQuoteFn = fn(pcs: &Vec<ProtoTychoState>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, aggb: f64, output_u_ethworth: f64) -> Vec<TradeResult>;

pub fn optifast(pcs: &Vec<ProtoTychoState>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, aggb: f64, output_u_ethworth: f64) -> Vec<TradeResult> {
    let mut trades = Vec::new();
    let start = aggb / TEN_MILLIONS; // No longer needed: / 10f64.powi(from.decimals as i32);
    log::info!(
        "Agg onchain liquidity balance for {} is {} (for 1 millionth => {}) | Output unit worth eth: {}",
        from.symbol,
        aggb,
        start,
        output_u_ethworth
    );
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

/**
 * Executes the optimizer for a given token pair and a set of pools.
 */
pub fn optimize(pcs: &Vec<ProtoTychoState>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, aggb: f64, output_u_ethworth: f64) -> Vec<TradeResult> {
    let start = aggb / TEN_MILLIONS;
    log::info!(
        "Agg onchain liquidity balance for {} is {} (for 1 millionth => {}) | Output unit worth eth: {}",
        from.symbol,
        aggb,
        start,
        output_u_ethworth
    );
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
 * ! We assume that => trade_base_to_quote = ask and trade_quote_to_base = bid
 */
pub fn midprice(trade_base_to_quote: TradeResult, trade_quote_to_base: TradeResult) -> MidPriceData {
    let ask = trade_base_to_quote.average_sell_price; // buy quote
    let bid = 1. / trade_quote_to_base.average_sell_price; // buy base
    let mid = (ask + bid) / 2.;
    let spread = (ask - bid).abs();
    let spread_pct = (spread / mid) * 100.;
    // log::info!(" - midprice: ask: {}", ask);
    // log::info!(" - midprice: bid: {}", bid);
    // log::info!(" - midprice: trade_quote_to_base.ratio: {}", trade_quote_to_base.ratio);
    // log::info!(" - midprice: mid: {}", mid);
    // log::info!(" - midprice: spread: {}", spread);
    // log::info!(" - midprice: spread_pct: {}", spread_pct);
    MidPriceData { ask, bid, mid, spread, spread_pct }
}

/// Check if a component has the desired tokens
pub fn matchcp(cptks: Vec<SrzToken>, tokens: Vec<SrzToken>) -> bool {
    tokens.iter().all(|token| cptks.iter().any(|cptk| cptk.address.eq_ignore_ascii_case(&token.address)))
}
