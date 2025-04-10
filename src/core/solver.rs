use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};
use tokio::time::Instant;

use crate::{
    data::fmt::SrzToken,
    maths::{self},
    types::{ProtoSimComp, TradeResult},
    utils::{self, r#static::maths::ONE_HD},
};

use super::book::remove_decreasing_price;

pub trait OrderbookSolver: Send + Sync {
    fn generate_steps(&self, liquidity: f64) -> Vec<f64>;
    /// Protosims contains the required functions to get the amount out of a swap
    fn optimize(&self, protosims: &[ProtoSimComp], steps: Vec<f64>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, price_from_to: f64, output_eth_worth: f64) -> Vec<TradeResult>;
}

// Default implementation

pub struct DefaultOrderbookSolver;

impl OrderbookSolver for DefaultOrderbookSolver {
    fn generate_steps(&self, liquidity: f64) -> Vec<f64> {
        exponential(liquidity)
    }

    fn optimize(&self, protosim: &[ProtoSimComp], steps: Vec<f64>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, price_from_to: f64, output_eth_worth: f64) -> Vec<TradeResult> {
        tracing::debug!("Default solver: optimize called with steps: {:?}", steps);
        optimize(protosim, steps, eth_usd, gas_price, from, to, price_from_to, output_eth_worth)
    }
}

pub struct CustomOrderbookSolver;

impl OrderbookSolver for CustomOrderbookSolver {
    fn generate_steps(&self, liquidity: f64) -> Vec<f64> {
        exponential(liquidity)
    }

    fn optimize(&self, protosim: &[ProtoSimComp], steps: Vec<f64>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, price_from_to: f64, output_eth_worth: f64) -> Vec<TradeResult> {
        // For custom logic, take the last available step if present.
        tracing::debug!("Custom solver: optimize called with steps: {:?}", steps);
        optimize(protosim, steps, eth_usd, gas_price, from, to, price_from_to, output_eth_worth)
    }
}

/// === Implementation ===

// Executes the optimizer for a given token pair and a set of pools.
/// Use the steps generated by function pointer
#[allow(clippy::too_many_arguments)]
pub fn optimize(protosim: &[ProtoSimComp], steps: Vec<f64>, eth_usd: f64, gas_price: u128, from: &SrzToken, to: &SrzToken, spot_price: f64, output_eth_worth: f64) -> Vec<TradeResult> {
    let trades: Vec<TradeResult> = steps
        .par_iter()
        .enumerate()
        .map(|(x, amount)| {
            let tmstp = Instant::now();
            let result = maths::opti::gradient(*amount, protosim, from.clone(), to.clone(), eth_usd, gas_price, spot_price, output_eth_worth);
            let elapsed = tmstp.elapsed().as_millis();
            let gas_cost = result.gas_costs_usd.iter().sum::<f64>();
            let sum_distribution = result.distribution.iter().sum::<f64>();
            let sum_distributed = result.distributed.iter().sum::<f64>();
            tracing::trace!(
                " - #{:<2} | In: {:.7} {}, Out: {:.7} {} at avg price {:.7} (vs spot_price {:.7}) | Price impact %: {:.4} | Gas cost {:.5}$ | Distribution: {:?} on {:.3} | Distributed: {:?} on {:.3} | Took: {} ms",
                x,
                result.amount,
                from.symbol,
                result.output,
                to.symbol,
                result.average_sell_price,
                spot_price,
                result.price_impact * ONE_HD,
                gas_cost,
                result.distribution,
                sum_distribution,
                result.distributed,
                sum_distributed,
                elapsed
            );
            result
        })
        .collect();

    // Current gradient optimization is not always the best solution and takes a lot of time, but it is a good starting point
    // Yet we remove trades that have a price impact not strictly increasing
    let size = trades.len();
    let (trades, x) = remove_decreasing_price(&trades);
    if x > 0 {
        tracing::debug!("Removed {} on {} trades with decreasing price.", x, size);
    }
    trades
}

/// Default steps function
/// This function generates a set of quoted amounts based on the aggregated liquidity of the pools.
/// Up to END_MULTIPLIER % of the aggregated liquidity, it generates a set of amounts using an exponential function with minimum delta percentage.
pub fn exponential(liquidity: f64) -> Vec<f64> {
    let start = liquidity / utils::r#static::maths::TEN_MILLIONS;
    let steps = maths::steps::expo(
        utils::r#static::maths::simu::COUNT,
        utils::r#static::maths::simu::START_MULTIPLIER,
        utils::r#static::maths::simu::END_MULTIPLIER,
        utils::r#static::maths::simu::END_MULTIPLIER * utils::r#static::maths::simu::MIN_EXP_DELTA_PCT,
    );
    steps.iter().map(|x| x * start).collect::<Vec<f64>>()
}
