use crate::{
    data::fmt::SrzToken,
    types::{ProtoTychoState, TradeResult},
    utils::r#static::maths::{BPD, FRACTION_REALLOC, MAX_ITERATIONS, ONE_HD},
};
use num_bigint::BigUint;
use num_traits::{ToPrimitive, Zero};
use tycho_simulation::models::Token;

/// A gradient-based optimizer that takes into account gas cost for activating an extra pool.
/// Only “activate” an additional pool if the net benefit (output after gas cost) exceeds a fixed activation penalty.
pub fn gradient(
    amount: f64, // human–readable amount (e.g. 100 meaning 100 ETH)
    pools: &[ProtoTychoState],
    tkinput: SrzToken,
    tkoutput: SrzToken,
    eth_usd: f64,       // ETH price in USD
    gas_price: u128,    // Gas price in wei (or converted to wei)
    spot_price: f64,    // Spot price (e.g. 0.0005 for USDC/ETH or 2000 for ETH/USDC)
    out_eth_worth: f64, // How much is one unit of tkoutput worth in ETH (e.g. 1.0 for ETH, 0.0005 for USDC)
) -> TradeResult {
    let tkinput = Token::from(tkinput.clone());
    let tkoutput = Token::from(tkoutput.clone());
    let amountpow = amount * 10f64.powi(tkinput.decimals as i32).round();
    let amountpow = BigUint::from(amountpow as u128);
    let num_pools = pools.len();

    // Fixed parameters
    let fraction = BigUint::from(FRACTION_REALLOC); // 10% reallocation fraction.
    let epsilon = &amountpow / BigUint::from(10_000u32); // finite difference step.
    let max_iterations = MAX_ITERATIONS;

    // 1. INITIAL CONCENTRATION:
    // Evaluate net output for each pool for the full amount (i.e. if used solely).
    // (Net output = gross output - gas cost converted to output token units.)
    let mut best_index = 0;
    let mut best_net_output = 0.0;
    for (i, pool) in pools.iter().enumerate() {
        if let Ok(result) = pool.protosim.get_amount_out(amountpow.clone(), &tkinput, &tkoutput) {
            let gross_output = result.amount.to_f64().unwrap_or(0.0);
            // Compute gas cost in ETH: (gas * gas_price) / 1e18, then convert to output token units:
            let gas_units: u128 = result.gas.to_string().parse::<u128>().unwrap_or(0);
            let gas_cost_eth = (gas_units * gas_price) as f64 / 1e18;
            let gas_cost_in_output = gas_cost_eth / out_eth_worth; // output token penalty
            let net_output = gross_output - gas_cost_in_output;
            if net_output > best_net_output {
                best_net_output = net_output;
                best_index = i;
            }
        }
    }
    // Start with 100% allocation in the best-performing pool.
    // Instead of starting with an equal split, we begin with a concentrated allocation
    let mut allocations = vec![BigUint::zero(); num_pools];
    allocations[best_index] = amountpow.clone();

    // 2 & 3. ITERATIVE REBALANCING WITH NET MARGINAL CALCULATION AND ACTIVATION PENALTY
    // In each iteration, compute the net marginal return for each pool.
    // For an inactive pool, subtract the fixed activation penalty from its marginal.
    for _iter in 0..max_iterations {
        let mut net_marginals: Vec<f64> = Vec::with_capacity(num_pools);
        for (i, pool) in pools.iter().enumerate() {
            let current_alloc = allocations[i].clone();
            // Evaluate net output at current allocation:
            let base = if let Ok(result) = pool.protosim.get_amount_out(current_alloc.clone(), &tkinput, &tkoutput) {
                let gross = result.amount.to_f64().unwrap_or(0.0);
                let gas_units: u128 = result.gas.to_string().parse::<u128>().unwrap_or(0);
                let gas_cost_eth = (gas_units * gas_price) as f64 / 1e18;
                let gas_cost_out = gas_cost_eth / out_eth_worth;
                gross - gas_cost_out
            } else {
                0.0
            };

            // Evaluate net output at (current_alloc + epsilon)
            let perturbed_alloc = &current_alloc + &epsilon;
            let perturbed = if let Ok(result) = pool.protosim.get_amount_out(perturbed_alloc.clone(), &tkinput, &tkoutput) {
                let gross = result.amount.to_f64().unwrap_or(0.0);
                let gas_units: u128 = result.gas.to_string().parse::<u128>().unwrap_or(0);
                let gas_cost_eth = (gas_units * gas_price) as f64 / 1e18;
                let gas_cost_out = gas_cost_eth / out_eth_worth;
                gross - gas_cost_out
            } else {
                0.0
            };

            let marginal = perturbed - base;

            let activation_penalty = if current_alloc.is_zero() {
                if let Ok(result) = pool.protosim.get_amount_out(amountpow.clone(), &tkinput, &tkoutput) {
                    let gas_units: u128 = result.gas.to_string().parse::<u128>().unwrap_or(0);
                    let gas_cost_eth = (gas_units * gas_price) as f64 / 1e18f64;
                    gas_cost_eth / out_eth_worth
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let adjusted_marginal = if current_alloc.is_zero() { marginal - activation_penalty } else { marginal };

            net_marginals.push(adjusted_marginal);
        }

        // Determine the best (highest net marginal) and worst (lowest net marginal) among active pools.
        // We only consider pools with nonzero allocation for worst-case; for best, we can consider inactive ones as potential new activations.

        // Errors
        // let (max_index, max_net_marginal) = net_marginals.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).unwrap();
        // // For the pool to lose allocation, we consider only currently active pools.
        // let active_indices: Vec<usize> = allocations.iter().enumerate().filter(|(_, a)| !a.is_zero()).map(|(i, _)| i).collect();
        // // Risk of unwrap errors
        // let (min_active_index, min_net_marginal) = active_indices.iter().map(|&i| (i, net_marginals[i])).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap_or_default();

        // For maximum:
        let (max_index, max_net_marginal) = match net_marginals.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)) {
            Some((idx, val)) => (idx, *val),
            None => (0, 0.0),
        };
        // For active indices:
        let active_indices: Vec<usize> = allocations.iter().enumerate().filter(|(_, a)| !a.is_zero()).map(|(i, _)| i).collect();
        // For minimum:
        let (min_active_index, min_net_marginal) = active_indices
            .iter()
            .map(|&i| (i, net_marginals[i]))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or_default();
        // If the net marginal difference is negligible, break.
        if (max_net_marginal - min_net_marginal).abs() < 1e-12 {
            break;
        }

        // Reallocate a fixed fraction from the worst-performing active pool to the best one.
        let reallocate_amount = &allocations[min_active_index] / &fraction;
        allocations[min_active_index] = &allocations[min_active_index] - &reallocate_amount;
        allocations[max_index] = &allocations[max_index] + &reallocate_amount;
    }

    // ------- Compute total net output (raw) and distribution -------
    let mut total_net_output: f64 = 0.0;
    let mut distribution: Vec<f64> = Vec::with_capacity(num_pools);
    let mut distributed: Vec<f64> = Vec::with_capacity(num_pools);
    let mut gas_costs_unit: Vec<u128> = Vec::with_capacity(num_pools);
    let mut gas_costs_usd: Vec<f64> = Vec::with_capacity(num_pools);
    let mut gas_costs_output: Vec<f64> = Vec::with_capacity(num_pools);
    for (i, pool) in pools.iter().enumerate() {
        let alloc = allocations[i].clone();
        if !alloc.is_zero() {
            if let Ok(result) = pool.protosim.get_amount_out(alloc.clone(), &tkinput, &tkoutput) {
                // Get the gross output as f64.
                let gross_output = result.amount.to_f64().unwrap_or(0.0);
                // Parse gas (as u128).
                let gas_units: u128 = result.gas.to_string().parse::<u128>().unwrap_or_default();
                gas_costs_unit.push(gas_units);
                // Compute gas cost in ETH: (gas_units * gas_price) / 1e18.
                let gas_cost_eth = (gas_units * gas_price) as f64 / 1e18f64;
                // Compute gas cost in USD if needed.
                let gas_cost_usd_val = gas_cost_eth * eth_usd;
                gas_costs_usd.push(gas_cost_usd_val);
                // Convert gas cost in ETH to output token units:
                let gas_cost_out = gas_cost_eth / out_eth_worth;
                gas_costs_output.push(gas_cost_out);
                // Compute net output = gross output minus gas cost in output tokens.
                let net_output = (gross_output - gas_cost_out).max(0.0); // <--- Ensure non-negative.
                total_net_output += net_output;
                // Also record distribution (as percentage of the total input).
                let alloc_f64 = alloc.to_f64().unwrap_or(0.0);
                let total_input_f = amountpow.to_f64().unwrap_or(1.0);
                let percent = (alloc_f64 * ONE_HD) / total_input_f;
                distribution.push((percent * ONE_HD).round() / ONE_HD);
                distributed.push(net_output);
            } else {
                distribution.push(0.);
                distributed.push(0.);
                gas_costs_unit.push(0);
                gas_costs_usd.push(0.);
                gas_costs_output.push(0.);
            }
        } else {
            // If the allocation is zero.
            distribution.push(0.);
            distributed.push(0.);
            gas_costs_unit.push(0);
            gas_costs_usd.push(0.);
            gas_costs_output.push(0.);
        }
    }

    // Convert final amounts to human–readable values using token multipliers.
    let tkinput_multiplier = 10f64.powi(tkinput.decimals as i32);
    let tkoutput_multiplier = 10f64.powi(tkoutput.decimals as i32);
    // Here, total_net_output is the sum (in output token units) of each pool's net output (gross output minus gas cost)
    // We convert that to a human–readable output:
    let output = total_net_output / tkoutput_multiplier;
    // Also, compute the effective ratio (unit price) as the net output per unit of input.
    // Note that amountpow is the total input (in smallest units), so we first convert it to f64:
    let input_f = amountpow.to_f64().unwrap_or(1.0);
    let average_sell_price = ((total_net_output * tkinput_multiplier) / input_f) / tkoutput_multiplier;
    let delta = (average_sell_price - spot_price).min(0.); // In theory it should never be < 0
    let price_impact = (((delta / spot_price) * BPD).round() / BPD).abs(); // In basis points

    let sum_distributed = distributed.iter().sum::<f64>();
    let distributed_base_bps = distributed.iter().map(|&x| (((x * ONE_HD) / sum_distributed) * ONE_HD).round() / ONE_HD).collect::<Vec<f64>>();
    TradeResult {
        amount,
        output,
        distribution,
        distributed: distributed_base_bps,
        gas_costs: gas_costs_unit,
        gas_costs_usd,
        average_sell_price,
        price_impact,
    }
}
