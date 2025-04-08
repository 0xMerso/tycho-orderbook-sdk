use std::collections::HashMap;

use crate::{
    data::fmt::{SrzProtocolComponent, SrzToken},
    utils::{self},
};

/// Sum the total liquidity of a pair of tokens.
/// @dev components Every similar components (= matching a pair)
pub fn depth(components: Vec<SrzProtocolComponent>, targets: Vec<SrzToken>, data: HashMap<String, HashMap<String, f64>>) -> HashMap<String, f64> {
    let mut cumulated = HashMap::new();
    targets.iter().for_each(|t| {
        cumulated.insert(t.clone().address.to_lowercase(), 0f64);
    });
    // Every component containing 'tokens'
    for cp in components.clone().iter() {
        if let Some(balances) = data.get(&cp.id) {
            for tk in targets.iter() {
                if let Some(balance) = balances.get(tk.address.to_lowercase().as_str()) {
                    // log::info!("Component {} has {} of token {}", cp.id, balance, tk.symbol);
                    let c = cumulated.get(tk.address.to_lowercase().as_str()).unwrap();
                    let new = c + balance;
                    cumulated.insert(tk.clone().address, new);
                }
            }
        }
    }
    cumulated
}

pub type AmountStepsFn = fn(liquidity: f64) -> Vec<f64>;

/// Default steps function
/// This function generates a set of quoted amounts based on the aggregated liquidity of the pools.
/// Up to END_MULTIPLIER % of the aggregated liquidity, it generates a set of amounts using an exponential function with minimum delta percentage.
pub fn exponential(liquidity: f64) -> Vec<f64> {
    let start = liquidity / utils::r#static::maths::TEN_MILLIONS;
    let steps = _expo(
        utils::r#static::maths::simu::COUNT,
        utils::r#static::maths::simu::START_MULTIPLIER,
        utils::r#static::maths::simu::END_MULTIPLIER,
        utils::r#static::maths::simu::END_MULTIPLIER * utils::r#static::maths::simu::MIN_EXP_DELTA_PCT,
    );
    steps.iter().map(|x| x * start).collect::<Vec<f64>>()
}

/// Generates `n_points` along an exponential curve between `start` and `end`.
/// # Arguments
/// * `n_points` - Number of points to generate.
/// * `start` - The starting value of the curve.
/// * `end` - The ending value of the curve.
/// # Returns
/// A vector of f64 values representing the points along the exponential curve.
fn _expo(n_points: usize, start: f64, end: f64, min_delta: f64) -> Vec<f64> {
    let lambda = 2.0; // parameter for the ease-in when start == 0
    let mut result = Vec::new();
    // Prevent division by zero if n_points == 1
    let divisor = if n_points > 1 { (n_points - 1) as f64 } else { 1.0 };
    // We'll store the last accepted value here to compare with the next candidate.
    let mut last_value: Option<f64> = None;
    for i in 0..n_points {
        let t = i as f64 / divisor;
        let value = if start == 0.0 {
            // Ease-in exponential: avoids division by zero when start is zero.
            let numerator = (lambda * t).exp() - 1.0;
            let denominator = lambda.exp() - 1.0;
            end * numerator / denominator
        } else {
            // Standard exponential interpolation.
            start * (end / start).powf(t)
        };
        if last_value.is_none() {
            // Always include the first point
            result.push(value);
            last_value = Some(value);
        } else if i == n_points - 1 {
            // Always include the last point
            result.push(value);
        } else if (value - last_value.unwrap()) >= min_delta {
            // Only keep points that differ by at least min_delta
            result.push(value);
            last_value = Some(value);
        }
    }

    result
}
