use crate::{
    data::fmt::{SrzTickList, SrzToken},
    types::{LiquidityTickAmounts, TickDataRange},
    utils::r#static::maths::UNISWAP_Q96,
};

pub fn compute_tick_data(tick: i32, tick_spacing: i32) -> TickDataRange {
    // Ensure a nonnegative remainder (works correctly for negative ticks, too)
    let delta = tick.rem_euclid(tick_spacing);
    let below = tick - delta;
    let above = below + tick_spacing;
    // Compute the square-root ratios as 1.0001^(tick/2)
    let sqrt_price_lower_f = 1.0001_f64.powf(below as f64 / 2.0);
    let sqrt_price_upper_f = 1.0001_f64.powf(above as f64 / 2.0);
    // Convert to Q64.96 fixed‑point format:
    let sqrt_price_lower = (sqrt_price_lower_f * (UNISWAP_Q96 as f64)).round() as u128;
    let sqrt_price_upper = (sqrt_price_upper_f * (UNISWAP_Q96 as f64)).round() as u128;
    TickDataRange {
        tick_lower: below,
        sqrt_price_lower,
        tick_upper: above,
        sqrt_price_upper,
    }
}

/**
 * Convert a tick to prices
 */
pub fn tick_to_prices(tick: i32, decimals_token0: u8, decimals_token1: u8) -> (f64, f64) {
    let raw_price = 1.0001_f64.powi(tick);
    let adjustment = 10_f64.powi((decimals_token0 as i32) - (decimals_token1 as i32));
    let price0to1 = 1.0 / (raw_price * adjustment);
    let price1to0 = 1.0 / price0to1;
    (price0to1, price1to0)
}

/**
 * Get the tick at the square root price
 * ! Unsure about float precision
 */
fn get_tick_at_sqrt_price(sqrt_price_x96: u128) -> i32 {
    // Here, sqrt_price_x96 is already the raw Q64.96 number.
    let sqrt_price = sqrt_price_x96 as f64 / UNISWAP_Q96 as f64;
    let tick = ((sqrt_price * sqrt_price).ln() / 1.0001_f64.ln()).floor();
    tick as i32
}

/**
 * Computes token amounts (in human-readable units) from liquidity given the current √price and tick range.
 * IMPORTANT: The function expects sqrt_price_x96 as f64 representing the raw Q64.96 value (i.e. the on-chain value)
 * and NOT a value computed on a tick boundary. This ensures the current price lies between the boundaries.
 *
 * ! The function might not working as expected
 */
pub fn derive(
    liquidity: i128,
    x96: f64, // the raw Q64.96 value as f64
    tick_low: i32,
    tick_high: i32,
    t0: SrzToken,
    t1: SrzToken,
    v: bool,
) -> LiquidityTickAmounts {
    // Compute the sqrt ratios at the boundaries: sqrt_ratio = 1.0001^(tick/2)
    let sqrt_ratio_a = 1.0001_f64.powf(tick_low as f64 / 2.0);
    let sqrt_ratio_b = 1.0001_f64.powf(tick_high as f64 / 2.0);
    // Compute current_tick directly from x96, since it's already Q64.96.
    let current_tick = get_tick_at_sqrt_price(x96 as u128); // ! This might not work as expected
    let sqrt_price = x96 / (UNISWAP_Q96 as f64); // Convert the raw Q64.96 number to a fraction.
    let adjustment = 10_f64.powi((t0.decimals as i32) - (t1.decimals as i32));
    let unadjusted_price = sqrt_price * sqrt_price;
    // Following the tick_to_prices convention, we assume:
    // - The raw price (from tick) is token1/token0.
    // - To get price0to1 (token0 per token1), take the reciprocal and apply the adjustment.
    let price0to1 = if unadjusted_price != 0.0 { 1.0 / (unadjusted_price * adjustment) } else { 0.0 };
    let price1to0 = if price0to1 != 0.0 { 1.0 / price0to1 } else { 0.0 };
    let mut _prefix = "";
    let (amount0, amount1) = if current_tick < tick_low {
        // Price is below the range: all liquidity is in token0.
        let amt0 = (liquidity as f64 * ((sqrt_ratio_b - sqrt_ratio_a) / (sqrt_ratio_a * sqrt_ratio_b))).floor();
        _prefix = "current_tick < tick_low, so amount0 computed:";
        (amt0, 0.0)
    } else if current_tick >= tick_high {
        // Price is above the range: all liquidity is in token1.
        let amt1 = (liquidity as f64 * (sqrt_ratio_b - sqrt_ratio_a)).floor();
        _prefix = "current_tick >= tick_high, so amount1 computed:";
        (0.0, amt1)
    } else {
        // Price is within the range: liquidity is split.
        let amt0 = (liquidity as f64 * ((sqrt_ratio_b - sqrt_price) / (sqrt_price * sqrt_ratio_b))).floor();
        let amt1 = (liquidity as f64 * (sqrt_price - sqrt_ratio_a)).floor();
        _prefix = "In-range: amount0 and amount1 computed:";
        (amt0, amt1)
    };

    let amount0div = amount0 / 10_f64.powi(t0.decimals as i32);
    let amount1div = amount1 / 10_f64.powi(t1.decimals as i32);
    if v {
        tracing::debug!("--- Tick very close to spot sprice ---");
        tracing::debug!("- Inputs: tick_low: {} | tick_high: {} | liquidity: {}", tick_low, tick_high, liquidity);
        tracing::debug!(
            " - current_tick: {} | Boundaries: sqrt_low: {:.2}, sqrt_high: {:.2}, actual: {:.2}",
            current_tick,
            sqrt_ratio_a,
            sqrt_ratio_b,
            sqrt_price
        );
        tracing::debug!(" - Price0to1: {:.7} | Price1to0: {:.7}", price0to1, price1to0);
        tracing::debug!(" - {} amount0 {:.7} | amount1 {:.7}", _prefix, amount0, amount1);
        tracing::debug!(" - {} : {:.6} | {}: {:.6}", t0.symbol, amount0div, t1.symbol, amount1div);
        tracing::debug!("--- Done for tick#{tick_low} ---")
    }

    LiquidityTickAmounts {
        index: tick_low,
        amount0: amount0div,
        amount1: amount1div,
        p0to1: price0to1,
        p1to0: price1to0,
    }
}

/// Computes the cumulative liquidity at a target tick by accumulating net liquidity deltas.
/// If target_tick > current_tick, adds net liquidity; if target_tick < current_tick, subtracts net liquidity.
pub fn compute_cumulative_liquidity(active_liquidity: i128, current_tick: i32, target_tick: i32, tick_list: &SrzTickList) -> i128 {
    let mut liquidity = active_liquidity;
    if target_tick > current_tick {
        for tick in tick_list.ticks.iter() {
            if tick.index > current_tick && tick.index <= target_tick {
                liquidity += tick.net_liquidity;
            }
        }
    } else if target_tick < current_tick {
        for tick in tick_list.ticks.iter() {
            if tick.index <= current_tick && tick.index > target_tick {
                liquidity -= tick.net_liquidity;
            }
        }
    }
    liquidity
}

/// Simulates available token amounts for each tick in the tick list in both directions (bid/ask).
/// For each tick, it calculates cumulative liquidity and then computes token amounts using derive().
pub fn ticks_liquidity(active: i128, current_tick: i32, tick_spacing: i32, tick_list: &SrzTickList, t0: SrzToken, t1: SrzToken, verbose: bool) -> Vec<LiquidityTickAmounts> {
    tracing::debug!("------- Computing liquidity across ticks -------");
    let mut output = vec![];
    for tick in tick_list.ticks.iter() {
        let target_tick = tick.index;
        // Compute cumulative liquidity at target_tick.
        let cum_liq = compute_cumulative_liquidity(active, current_tick, target_tick, tick_list);
        // Determine the range boundaries and simulate an "active" sqrt price.
        let (range_low, range_high, simulated_sqrt_price_x96): (i32, i32, f64) = if target_tick < current_tick {
            let low = target_tick;
            let high = target_tick + tick_spacing;
            let _sqrt_low = 1.0001_f64.powf(low as f64 / 2.0) * (UNISWAP_Q96 as f64);
            let sqrt_high = 1.0001_f64.powf(high as f64 / 2.0) * (UNISWAP_Q96 as f64);
            // let mid = (sqrt_low + sqrt_high) / 2.0;
            let mid = sqrt_high; // ! This might not work as expected. Use this if you want 0 on the token outside
            (low, high, mid)
        } else if target_tick > current_tick {
            let low = target_tick - tick_spacing;
            let high = target_tick;
            let sqrt_low = 1.0001_f64.powf(low as f64 / 2.0) * (UNISWAP_Q96 as f64);
            let _sqrt_high = 1.0001_f64.powf(high as f64 / 2.0) * (UNISWAP_Q96 as f64);
            // let mid = (sqrt_low + sqrt_high) / 2.0;
            let mid = sqrt_low; // ! This might not work as expected. Use this if you want 0 on the token outside
            (low, high, mid)
        } else {
            // For current tick, use the actual pool's sqrt price from the tick info.
            (current_tick, current_tick, tick.sqrt_price.to_string().parse::<f64>().unwrap())
        };
        // Enable verbose logging if target_tick is within ±10×tick_spacing of current_tick.
        let verbose = (target_tick - current_tick).abs() <= 5 * tick_spacing;
        let (p0to1, p1to0) = tick_to_prices(target_tick, t0.decimals as u8, t1.decimals as u8);
        // Compute token amounts using the simulated sqrt price.
        let tick_amounts = derive(cum_liq, simulated_sqrt_price_x96, range_low, range_high, t0.clone(), t1.clone(), verbose);
        output.push(tick_amounts.clone());
        if verbose {
            tracing::debug!(
                "Tick {} within [{}, {}] | {}: {:.6}, {}: {:.6} | p0to1 = {:.6}, p1to0 = {:.6}",
                target_tick,
                range_low,
                range_high,
                t0.symbol,
                tick_amounts.amount0,
                t1.symbol,
                tick_amounts.amount1,
                p0to1,
                p1to0
            );
        }
    }
    output
}

/**
 * Filter and classify liquidity ticks
 */
pub fn filter_and_classify_ticks(
    ticks: Vec<LiquidityTickAmounts>,
    current_tick: i32,
    current_tick_lower: i32,
    current_price0to1: f64,
    current_price1to0: f64,
) -> (Vec<LiquidityTickAmounts>, Vec<LiquidityTickAmounts>) {
    let mut bids = Vec::new();
    let mut asks = Vec::new();

    for tick in ticks {
        if tick.index > 870_000 || tick.index < -870_000 {
            continue;
        }
        if tick.p0to1 > current_price0to1 * 100.0 || tick.p0to1 < current_price0to1 / 100.0 {
            continue;
        }
        if tick.p1to0 > current_price1to0 * 100.0 || tick.p1to0 < current_price1to0 / 100.0 {
            continue;
        }
        if tick.index == current_tick_lower {
            continue;
        }
        if tick.index <= current_tick {
            bids.push(tick);
        } else {
            asks.push(tick);
        }
    }

    (bids, asks)
}
