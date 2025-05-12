use crate::{
    data::fmt::SrzToken,
    types::{ProtoSimComp, TradeResult},
    utils::r#static::maths::{BPD, FRACTION_REALLOC, MAX_ITERATIONS, MIN_CONVERGENCE_THRESHOLD, ONE_HD},
};
use num_bigint::BigUint;
use num_traits::{ToPrimitive, Zero};
use tycho_simulation::models::Token;

#[allow(clippy::too_many_arguments)]
pub fn gradient(
    amount: f64, // human–readable amount (e.g. 100 meaning 100 ETH)
    pools: &[ProtoSimComp],
    tkinput: SrzToken,
    tkoutput: SrzToken,
    eth_usd: f64,       // ETH price in USD
    gas_price: u128,    // Gas price in wei (or converted to wei)
    spot_price: f64,    // Spot price (e.g. 0.0005 for USDC/ETH or 2000 for ETH/USDC)
    out_eth_worth: f64, // How much is one unit of tkoutput worth in ETH
) -> TradeResult {
    // Convert input tokens to Token struct (assuming Token::from is infallible)
    let tkinput = Token::from(tkinput.clone());
    let tkoutput = Token::from(tkoutput.clone());
    // Calculate amount in smallest unit. We first multiply then round.
    let amount_scaled = amount * 10f64.powi(tkinput.decimals as i32);
    let amount_scaled = amount_scaled.round();
    let amountpow = BigUint::from(amount_scaled as u128);
    let num_pools = pools.len();

    // Fixed parameters.
    let fraction = BigUint::from(FRACTION_REALLOC); // e.g., 10% reallocation fraction.
    let epsilon = &amountpow / BigUint::from(10_000u32); // finite difference step.

    let max_iterations = MAX_ITERATIONS;

    // 1. INITIAL CONCENTRATION:
    let mut best_index = 0;
    let mut best_net_output = 0.0;
    for (i, pool) in pools.iter().enumerate() {
        if let Ok(result) = pool.protosim.get_amount_out(amountpow.clone(), &tkinput, &tkoutput) {
            // let gross_tokens = result.amount.to_f64().unwrap_or(0.0); // [old]
            let gross_tokens = result.amount.to_f64().unwrap_or(0.0) / 10f64.powi(tkoutput.decimals as i32); // [new]
            let gas_units: u128 = result.gas.to_string().parse::<u128>().unwrap_or_default();
            let gas_cost_eth = (gas_units.saturating_mul(gas_price)) as f64 / 1e18;
            let gas_cost_in_output = gas_cost_eth / out_eth_worth;
            let net_output = gross_tokens - gas_cost_in_output;
            if net_output > best_net_output {
                best_net_output = net_output;
                best_index = i;
            }
        }
    }
    // Start with 100% allocation in the best-performing pool.
    let mut allocations = vec![BigUint::zero(); num_pools];
    allocations[best_index] = amountpow.clone();

    // 2. ITERATIVE REBALANCING.
    for _iter in 0..max_iterations {
        let mut net_marginals: Vec<f64> = Vec::with_capacity(num_pools);
        for pool in pools.iter() {
            let current_alloc = allocations[net_marginals.len()].clone();
            let base = if let Ok(result) = pool.protosim.get_amount_out(current_alloc.clone(), &tkinput, &tkoutput) {
                // let gross_tokens = result.amount.to_f64().unwrap_or(0.0); // [old]
                let gross_tokens = result.amount.to_f64().unwrap_or(0.0); // [new]
                let gas_units: u128 = result.gas.to_string().parse::<u128>().unwrap_or_default();
                let gas_cost_eth = (gas_units.saturating_mul(gas_price)) as f64 / 1e18;
                let gas_cost_out = gas_cost_eth / out_eth_worth;
                gross_tokens - gas_cost_out
            } else {
                0.0
            };

            let perturbed_alloc = &current_alloc + &epsilon;
            let perturbed = if let Ok(result) = pool.protosim.get_amount_out(perturbed_alloc.clone(), &tkinput, &tkoutput) {
                // let gross_tokens = result.amount.to_f64().unwrap_or(0.0); // [old]
                let gross_tokens = result.amount.to_f64().unwrap_or(0.0); // [new]
                let gas_units: u128 = result.gas.to_string().parse::<u128>().unwrap_or_default();
                let gas_cost_eth = (gas_units.saturating_mul(gas_price)) as f64 / 1e18;
                let gas_cost_out = gas_cost_eth / out_eth_worth;
                gross_tokens - gas_cost_out
            } else {
                0.0
            };

            let marginal = perturbed - base;
            let activation_penalty = if current_alloc.is_zero() {
                if let Ok(step_result) = pool.protosim.get_amount_out(epsilon.clone(), &tkinput, &tkoutput) {
                    // ⚡ only charge gas on the *increment* ε, not the whole trade
                    let gas_units: u128 = step_result.gas.to_string().parse::<u128>().unwrap_or_default();
                    let gas_cost_eth = (gas_units.saturating_mul(gas_price)) as f64 / 1e18;
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

        // Determine the best (maximum) net marginal.
        let (max_index, max_net_marginal) = match net_marginals.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)) {
            Some((idx, &val)) => (idx, val),
            None => (0, 0.0),
        };

        // Consider only active (nonzero) allocations for the worst-case.
        let active_indices: Vec<usize> = allocations.iter().enumerate().filter(|(_, alloc)| !alloc.is_zero()).map(|(i, _)| i).collect();
        let (min_active_index, min_net_marginal) = active_indices
            .iter()
            .map(|&i| (i, net_marginals[i]))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, 0.0));

        if (max_net_marginal - min_net_marginal).abs() < MIN_CONVERGENCE_THRESHOLD {
            break;
        }

        // Reallocate a fixed fraction from the worst-performing active pool to the best one.
        let reallocate_amount = &allocations[min_active_index] / &fraction;
        // Ensure that we do not underflow.
        if allocations[min_active_index] < reallocate_amount {
            allocations[min_active_index] = BigUint::zero();
        } else {
            allocations[min_active_index] = &allocations[min_active_index] - &reallocate_amount;
        }
        allocations[max_index] = &allocations[max_index] + &reallocate_amount;
    }

    // ------- Compute final outputs and distribution -------
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
                // let gross_tokens = result.amount.to_f64().unwrap_or(0.0); // [old]
                let gross_tokens = result.amount.to_f64().unwrap_or(0.0); // [new]
                let gas_units: u128 = result.gas.to_string().parse::<u128>().unwrap_or_default();
                gas_costs_unit.push(gas_units);
                let gas_cost_eth = (gas_units.saturating_mul(gas_price)) as f64 / 1e18;
                let gas_cost_usd_val = gas_cost_eth * eth_usd;
                gas_costs_usd.push(gas_cost_usd_val);
                let gas_cost_out = gas_cost_eth / out_eth_worth;
                gas_costs_output.push(gas_cost_out);
                let net_output = (gross_tokens - gas_cost_out).max(0.0);
                total_net_output += net_output;
                let alloc_f64 = alloc.to_f64().unwrap_or(0.0);
                let total_input_f = amountpow.to_f64().unwrap_or(1.0);
                let percent = (alloc_f64 * ONE_HD) / total_input_f;
                distribution.push((percent * ONE_HD).round() / ONE_HD);
                distributed.push(net_output);
            } else {
                distribution.push(0.0);
                distributed.push(0.0);
                gas_costs_unit.push(0);
                gas_costs_usd.push(0.0);
                gas_costs_output.push(0.0);
            }
        } else {
            distribution.push(0.0);
            distributed.push(0.0);
            gas_costs_unit.push(0);
            gas_costs_usd.push(0.0);
            gas_costs_output.push(0.0);
        }
    }

    let tkinput_multiplier = 10f64.powi(tkinput.decimals as i32);
    let tkoutput_multiplier = 10f64.powi(tkoutput.decimals as i32);
    let output = total_net_output / tkoutput_multiplier;
    let input_f = amountpow.to_f64().unwrap_or(1.0);
    let average_sell_price = ((total_net_output * tkinput_multiplier) / input_f) / tkoutput_multiplier;

    // Price impact calculation
    let delta = average_sell_price - spot_price;
    let price_impact = ((delta / spot_price) * BPD).round() / BPD;

    let sum_distributed: f64 = distributed.iter().sum();
    let distributed_base_bps: Vec<f64> = distributed.iter().map(|&x| (((x * ONE_HD) / sum_distributed) * ONE_HD).round() / ONE_HD).collect();

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
