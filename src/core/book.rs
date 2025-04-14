use chrono::DateTime;
use tycho_simulation::models::Token;

use crate::{
    core::{
        client::{self, build_tycho_client},
        gas,
    },
    data::fmt::{SrzProtocolComponent, SrzToken},
    maths::{self},
    types::{MidPriceData, Network, Orderbook, OrderbookRequestParams, ProtoSimComp, TradeResult},
    utils::{self},
};
use std::{
    collections::HashMap,
    time::{Duration, UNIX_EPOCH},
};

use super::solver::OrderbookSolver; // Ensure Rayon is in your dependencies.

/// @notice Reading 'state' from Redis DB while using TychoStreamState state and functions to compute/simulate might create a inconsistency
/// @notice It's assumed that the first token is the base and the second is the quote, so bid = 'buy base', and ask = 'sell base'. It's the responsibility of the caller to ensure this.
#[allow(clippy::too_many_arguments)]
pub async fn build<S: OrderbookSolver>(
    solver: S,
    network: Network,
    tycho_token_api: Option<String>,
    state: Vec<ProtoSimComp>,
    tokens: Vec<SrzToken>,
    query: OrderbookRequestParams,
    base_worth_eth: f64,
    quote_worth_eth: f64,
) -> Result<Orderbook, anyhow::Error> {
    tracing::debug!("Building orderbook ... Got {} pools to compute for pair: '{}'", state.len(), query.tag);
    let mut pools = Vec::new();
    let mut prices_base_to_quote = vec![];
    let mut prices_quote_to_base = vec![];
    let srzt0 = tokens[0].clone();
    let srzt1 = tokens[1].clone();
    let t0 = Token::from(srzt0.clone());
    let t1 = Token::from(srzt1.clone());
    let (base, quote) = (t0, t1);
    let mut base_lqdty = vec![];
    let mut quote_lqdty = vec![];
    let mut balances = HashMap::new();

    match build_tycho_client(&network, tycho_token_api.clone()).await {
        Ok(client) => {
            for pdata in state.clone() {
                pools.push(pdata.clone());
                let proto = pdata.protosim.clone();
                let price_base_to_quote = proto.spot_price(&base, &quote).unwrap_or_default();
                let price_quote_to_base = proto.spot_price(&quote, &base).unwrap_or_default();
                let d = UNIX_EPOCH + Duration::from_secs(pdata.component.last_updated_at);
                let datetime = DateTime::<chrono::Utc>::from(d);
                let timestamp = datetime.format("%Y-%m-%d %H:%M:%S").to_string();

                prices_base_to_quote.push(price_base_to_quote);
                prices_quote_to_base.push(price_quote_to_base);
                tracing::trace!(
                    "- Pool: {} | {} | Spot price for {}-{} => price_base_to_quote = {} and price_quote_to_base = {} | Fee = {} | Last updated at {}",
                    pdata.component.id,
                    pdata.component.protocol_type_name,
                    base.symbol,
                    quote.symbol,
                    price_base_to_quote,
                    price_quote_to_base,
                    pdata.component.fee,
                    timestamp
                );
                if let Some(cpbs) = client::get_component_balances(&client, network.clone(), pdata.component.id.clone(), pdata.component.protocol_system.clone()).await {
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
                } else {
                    base_lqdty.push(0f64);
                    quote_lqdty.push(0f64);
                    balances.insert(pdata.component.id.clone().to_lowercase(), HashMap::new());
                }
            }
            let cps: Vec<SrzProtocolComponent> = pools.clone().iter().map(|p| p.component.clone()).collect();
            let aggregated = maths::steps::depth(cps.clone(), tokens.clone(), balances.clone());
            let avg_price_base_to_quote = prices_base_to_quote.iter().sum::<f64>() / prices_base_to_quote.len() as f64;
            let avg_price_quote_to_base = prices_quote_to_base.iter().sum::<f64>() / prices_quote_to_base.len() as f64; // Ponderation by TVL ?
            tracing::trace!("Average price 0to1: {} | Average price 1to0: {}", avg_price_base_to_quote, avg_price_quote_to_base);
            match simulate(
                solver,
                network.clone(),
                pools.clone(),
                tokens,
                query.clone(),
                aggregated.clone(),
                base_worth_eth,
                quote_worth_eth,
                avg_price_base_to_quote,
                avg_price_quote_to_base,
            )
            .await
            {
                Ok(mut pso) => {
                    pso.prices_base_to_quote = prices_base_to_quote;
                    pso.prices_quote_to_base = prices_quote_to_base;
                    pso.base_lqdty = base_lqdty.clone();
                    pso.quote_lqdty = quote_lqdty.clone();
                    tracing::debug!("Done. Returning simulated orderbook for pair (base-quote) => '{}-{}'", base.symbol, quote.symbol);
                    Ok(pso)
                }
                Err(e) => {
                    tracing::error!("Error while simulating orderbook: {}", e);
                    Err(anyhow::anyhow!("Error while simulating orderbook: {}", e))
                }
            }
        }
        Err(e) => {
            tracing::error!("Error while building Tycho client: {}", e);
            Err(anyhow::anyhow!("Error while building Tycho client: {}", e))
        }
    }
}

/// Optimizes a trade for a given pair of tokens and a set of pools.
/// The function generates a set of test amounts for ETH and USDC, then runs the optimizer for each amount.
/// The optimizer uses a simple gradient-based approach to move a fixed fraction of the allocation from the pool with the lowest marginal return to the one with the highest.
/// If the query specifies a specific token to sell with a specific amount, the optimizer will only run for that token and amount.
#[allow(clippy::too_many_arguments)]
pub async fn simulate<S: OrderbookSolver>(
    solver: S,
    network: Network,
    pcsdata: Vec<ProtoSimComp>,
    tokens: Vec<SrzToken>,
    body: OrderbookRequestParams,
    balances: HashMap<String, f64>,
    base_worth_eth: f64,
    quote_worth_eth: f64,
    price_base_to_quote: f64,
    price_quote_to_base: f64,
) -> Result<Orderbook, anyhow::Error> {
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("Time went backwards").as_secs();
    let eth_worth_usd = client::get_eth_usd_chainlink(network.rpc.clone(), network.chainlink.clone()).await.unwrap_or(2000.);
    let gas_price = gas::gas_price(network.rpc.clone()).await;
    let latest = client::get_latest_block(network.rpc.clone()).await;
    let base = tokens[0].clone();
    let quote = tokens[1].clone();

    let total_balance_base = match balances.iter().find(|x| x.0.to_lowercase() == base.address.to_lowercase()) {
        Some(val) => val.1,
        None => return Err(anyhow::anyhow!("Base balance not found for token {}", base.address)),
    };
    let total_balance_quote = match balances.iter().find(|x| x.0.to_lowercase() == quote.address.to_lowercase()) {
        Some(val) => val.1,
        None => return Err(anyhow::anyhow!("Quote balance not found for token {}", quote.address)),
    };

    let total_balance_base_worth_usd = (total_balance_base) * base_worth_eth * eth_worth_usd;
    let total_balance_quote_worth_usd = (total_balance_quote) * quote_worth_eth * eth_worth_usd;
    let base_to_quote_liquidity_ratio = total_balance_base_worth_usd / total_balance_quote_worth_usd;
    let base_liquidity_share = total_balance_base_worth_usd / (total_balance_base_worth_usd + total_balance_quote_worth_usd);

    // E.g.: Liquidity ratio for WBTC-USDT: Agg Base worth: 41728361.72503823 $ | Agg Quote worth: 19582431.73275704 $ | base_to_quote_liquidity_ratio: 2.130908065683997 | base_liquidity_share: 0.6806038443094515
    // = 41 728 361 $ / 19 582 431 $
    tracing::debug!(
        "Liquidity ratio for {}-{}: Agg Base worth: {:.2} $ | Agg Quote worth: {:.2} $ | base_to_quote_liquidity_ratio: {:.2} | base_liquidity_share: {:.2}",
        base.symbol,
        quote.symbol,
        total_balance_base_worth_usd,
        total_balance_quote_worth_usd,
        base_to_quote_liquidity_ratio,
        base_liquidity_share
    );

    tracing::debug!(
        "🔎 Simu Opti | Network: {} | ETH is worth {} in USD | Got {} pools to optimize for pair: {}-{} with aggregated balancess {:.4} and {:.4}",
        network.name,
        eth_worth_usd,
        pcsdata.len(),
        base.symbol,
        quote.symbol,
        total_balance_base,
        total_balance_quote
    );

    // --- Need to adjust the aggregated base and quote liquidity to compute a balanced orderbook. Shared common denominator is USD value
    let adjusted_total_balance_base = total_balance_base / base_to_quote_liquidity_ratio;
    let adjusted_total_balance_quote = *total_balance_quote;
    tracing::debug!(
        "Adjusted aggregated base: {:.4} | Adjusted aggregated quote: {:.4}",
        adjusted_total_balance_base,
        adjusted_total_balance_quote
    );

    let pools = pcsdata.iter().map(|x| x.component.clone()).collect::<Vec<SrzProtocolComponent>>();
    let amount_eth = utils::r#static::maths::BEST_BID_ASK_ETH_BPS / utils::r#static::maths::BPD; // 1/100 of ETH = ~2$ (for 2000$ ETH)
    let amount_test_best_base_to_quote = amount_eth / base_worth_eth;
    let amount_test_best_quote_to_base = amount_eth / quote_worth_eth;
    let best_base_to_quote = compute_best_trade(&pcsdata, eth_worth_usd, gas_price, &base, &quote, amount_test_best_base_to_quote, price_base_to_quote, quote_worth_eth);
    let best_quote_to_base = compute_best_trade(&pcsdata, eth_worth_usd, gas_price, &quote, &base, amount_test_best_quote_to_base, price_quote_to_base, base_worth_eth);
    let mpd_base_to_quote = derive_mid_price(best_base_to_quote.clone(), best_quote_to_base.clone());
    let mpd_quote_to_base = derive_mid_price(best_quote_to_base.clone(), best_base_to_quote.clone());

    let tag = format!("{}-{}", base.address.to_lowercase(), quote.address.to_lowercase());
    let mut result = Orderbook {
        tag,
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
        eth_usd: eth_worth_usd,
        mpd_base_to_quote: mpd_base_to_quote.clone(),
        mpd_quote_to_base: mpd_quote_to_base.clone(),
        base_worth_eth,
        quote_worth_eth,
        // Optional, but still usefull
        aggregated_balance_base_worth_usd: total_balance_base_worth_usd,
        aggregated_balance_quote_worth_usd: total_balance_quote_worth_usd,
    };
    match body.point {
        Some(point) => {
            tracing::trace!(" 🎯 Partial Optimisation: input: {} and amount: {}", point.input, point.amount);
            if point.input.to_lowercase() == base.address.to_lowercase() {
                result.bids = vec![maths::opti::gradient(
                    point.amount,
                    &pcsdata,
                    base.clone(),
                    quote.clone(),
                    eth_worth_usd,
                    gas_price,
                    price_base_to_quote,
                    quote_worth_eth,
                )];
            } else if point.input.to_lowercase() == quote.address.to_lowercase() {
                result.asks = vec![maths::opti::gradient(
                    point.amount,
                    &pcsdata,
                    quote.clone(),
                    base.clone(),
                    eth_worth_usd,
                    gas_price,
                    price_quote_to_base,
                    base_worth_eth,
                )];
            }
        }
        None => {
            let steps = solver.generate_steps(adjusted_total_balance_base);
            let steps: Vec<f64> = steps.iter().cloned().filter(|&s| s > amount_test_best_base_to_quote * 3.).collect();
            let bids = solver.optimize(&pcsdata, steps.clone(), eth_worth_usd, gas_price, &base, &quote, price_base_to_quote, quote_worth_eth);
            result.bids = bids;
            tracing::trace!(" 🔄  Bids done, now switching to asks");
            let steps = solver.generate_steps(adjusted_total_balance_quote);
            let steps: Vec<f64> = steps.iter().cloned().filter(|&s| s > amount_test_best_quote_to_base * 3.).collect();
            let asks = solver.optimize(&pcsdata, steps.clone(), eth_worth_usd, gas_price, &quote, &base, price_quote_to_base, base_worth_eth);
            result.asks = asks;
        }
    }
    Ok(result)
}

/// Computes the mid price for a given token pair
/// We cannot replicate the logic of a classic orderbook as we don't have best bid/ask exacly
/// In theory it would be : Mid Price = (Best Bid Price + Best Ask Price) / 2
/// Applied to AMM, we choose to use a small amountIn = 1 / TEN_MILLIONS of the aggregated liquidity
/// Doing that for 0to1 and 1to0 we have our best bid/ask, then we can compute the mid price
/// --- --- --- --- ---
/// Amount out is net of gas cost
pub fn compute_best_trade(pcs: &[ProtoSimComp], eth_worth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, amount: f64, spot_price: f64, output_u_ethworth: f64) -> TradeResult {
    tracing::debug!(" - 🥇 Computing best price for {} (amount in = {})", from.symbol, amount);
    let result = maths::opti::gradient(amount, pcs, from.clone(), to.clone(), eth_worth_usd, gas_price, spot_price, output_u_ethworth);
    tracing::trace!(
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

/// Computes the mid price for a given token pair using the best bid and ask
/// ! We assume that => trade_base_to_quote = ask and trade_quote_to_base = bid
pub fn derive_mid_price(trade_base_to_quote: TradeResult, trade_quote_to_base: TradeResult) -> MidPriceData {
    let amount = trade_base_to_quote.amount;
    let received = trade_base_to_quote.output;
    let distribution = trade_base_to_quote.distribution.clone();
    let ask = trade_base_to_quote.average_sell_price; // buy quote
    let bid = 1. / trade_quote_to_base.average_sell_price; // buy base
    let mid = (ask + bid) / 2.;
    let spread = (ask - bid).abs();
    let spread_pct = (spread / mid) * 100.;
    MidPriceData {
        amount,
        received,
        distribution,
        ask,
        bid,
        mid,
        spread,
        spread_pct,
    }
}

/// Check if a component has the desired tokens
pub fn matchcp(cptks: Vec<SrzToken>, tokens: Vec<SrzToken>) -> bool {
    tokens.iter().all(|token| cptks.iter().any(|cptk| cptk.address.eq_ignore_ascii_case(&token.address)))
}

/// Removes trades with decreasing price
/// ! [WIP] We keep the 5 first trades because it make sense to have a decreasing price due to gas
/// Temporarily, need a better convex optimization function
/// Example: [0.1, 0.4, 0.3, 0.5] => [0.1, 0.4, 0.5]
pub fn remove_decreasing_price(items: &[TradeResult]) -> (Vec<TradeResult>, usize) {
    if items.is_empty() {
        return (Vec::new(), 0);
    }

    // Process the first five items (or all if fewer than five)
    let (head, tail) = items.split_at(items.len().min(5));
    let mut filtered = Vec::new();
    if let Some(first) = head.first() {
        filtered.push(first.clone());
        for item in head.iter().skip(1) {
            if let Some(last) = filtered.last() {
                // Only push the item if its average_sell_price is less than the last one
                if item.average_sell_price < last.average_sell_price {
                    filtered.push(item.clone());
                }
            }
        }
    }

    // Append the remaining items after the fifth, unfiltered
    filtered.extend_from_slice(tail);

    // The count is still the difference between original length and the filtered length
    let count = items.len() - filtered.len();
    (filtered, count)
}
