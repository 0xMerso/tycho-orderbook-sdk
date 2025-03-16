use tycho_simulation::{models::Token, protocol::state::ProtocolSim};

use crate::shd::{
    data::fmt::SrzToken,
    types::{ProtoTychoState, TradeResult},
};
use num_bigint::BigUint;
use num_traits::Zero;
use std::collections::HashMap;

/**
 * A very simple gradient-based optimizer that uses fixed iterations (100 max) and
 * moves a fixed fraction (10%) of the allocation from the pool with the lowest marginal
 * return to the one with the highest.
 * All arithmetic is done with BigUint.
 */
pub fn optimizer(
    input: f64, // human–readable input (e.g. 100 meaning 100 ETH)
    pools: &Vec<ProtoTychoState>,
    _balances: &HashMap<String, u128>,
    _ethusd: f64,
    token_in: SrzToken,
    token_out: SrzToken,
) -> TradeResult {
    // Convert tokens to tycho-simulation tokens
    let token_in = Token::from(token_in.clone());
    let token_out = Token::from(token_out.clone());
    let inputpow = input * 10f64.powi(token_in.decimals as i32).round(); // ?

    let inputpow = BigUint::from(inputpow as u128);
    let size = pools.len();
    let sizebg = BigUint::from(size as u32);
    let mut allocations: Vec<BigUint> = vec![&inputpow / &sizebg; size]; // Which is naive I guess

    // @notice epsilon is key here. It tells us the marginal benefit of giving a little more to that pool. The smaller epsilon is, the more accurately we capture that local behavior
    let epsilon = &inputpow / BigUint::from(10_000u32); // Choose a fixed epsilon for finite difference. May 1e9 is better, IDK.
    let max_iterations = 50u32; // We'll run a maximum of 100 iterations.
    let tolerance = BigUint::zero(); // Tolerance: if the difference between max and min marginal is zero.
    for _iter in 0..max_iterations {
        // Compute marginal returns for each pool as: f(x+epsilon) - f(x).
        let mut marginals: Vec<BigUint> = Vec::with_capacity(size);
        // If the difference between the best and worst marginal return becomes zero (or falls below a tiny tolerance),
        // then the algorithm stops early because it has “converged” on an allocation where no pool can provide a better extra return than any other.
        for (i, pool) in pools.iter().enumerate() {
            let current_alloc = allocations[i].clone();
            // log::info!("Current allocation for pool {}: {} | TokenIn {} TokenOut {}", i, current_alloc, token_in.address, token_out.address.clone());
            let result_got = pool.protosim.get_amount_out(current_alloc.clone(), &token_in, &token_out);
            let amount_got = if result_got.is_err() {
                // log::error!("get_amount_out on pool #{} at {} failed", i, pool.component.id);
                BigUint::ZERO
            } else {
                result_got.unwrap().amount
            };
            let result_esplison_got = pool.protosim.get_amount_out(&current_alloc + &epsilon, &token_in, &token_out);
            let amount_esplison_got = if result_esplison_got.is_err() {
                // log::error!("get_amount_out on pool #{} at {} failed", i, pool.component.id);
                BigUint::ZERO
            } else {
                result_esplison_got.unwrap().amount
            };
            let marginal = if amount_esplison_got > amount_got { &amount_esplison_got - &amount_got } else { BigUint::zero() };
            marginals.push(marginal);
        }
        // Identify pools with maximum and minimum marginals.
        let (max, max_marginal) = marginals.iter().enumerate().max_by(|a, b| a.1.cmp(b.1)).unwrap();
        let (mini, min_marginal) = marginals.iter().enumerate().min_by(|a, b| a.1.cmp(b.1)).unwrap();
        // If difference is zero (or below tolerance), stop.
        if max_marginal.clone() - min_marginal.clone() <= tolerance {
            // log::info!("Converged after {} iterations", iter);
            break; // ? If I'm correct in theory it will never converge, unless we take a very small epsilon that would make no difference = convergence
        }
        // Reallocate 10% of the allocation from the pool with the lowest marginal.
        // => Moving a fixed fraction (10%) of the allocation from the worst-performing pool to the best-performing one
        // Too high a percentage might cause the allocation to swing too quickly, overshooting the optimal balance.
        // Too low a percentage would make convergence very slow.
        let fraction = BigUint::from(10u32);
        let adjusted = &allocations[mini] / &fraction;
        allocations[mini] = &allocations[mini] - &adjusted;
        allocations[max] = &allocations[max] + &adjusted;
        // Once the iterations finish, the optimizer:
        // - Computes the total output by summing the outputs from all pools using the final allocations.
        // - Calculates the percentage of the total input that was allocated to each pool.
        // log::info!("Iteration {}: Pool {} marginal = {} , Pool {} marginal = {}, transfer = {}", iter, max, max_marginal, mini, min_marginal, adjusted);
    }

    // Define minimum allocation per pool as 5% of total input.
    let min_alloc = &inputpow * BigUint::from(5u32) / BigUint::from(100u32);

    // Identify surplus from pools below threshold and set their allocation to zero.
    let mut surplus = BigUint::zero();
    for alloc in allocations.iter_mut() {
        if *alloc < min_alloc {
            surplus += alloc.clone();
            *alloc = BigUint::zero();
        }
    }

    // Sum the allocation that is above threshold (i.e. pools that will receive the surplus).
    let total_valid: BigUint = allocations.iter().filter(|alloc| **alloc > BigUint::zero()).fold(BigUint::zero(), |acc, x| acc + x);

    // Reallocate the surplus proportionally to the valid pools.
    if total_valid > BigUint::zero() && surplus > BigUint::zero() {
        for alloc in allocations.iter_mut() {
            if *alloc > BigUint::zero() {
                // Multiply surplus by the current allocation and then divide by total_valid.
                let additional = (&surplus * alloc.clone()) / &total_valid;
                *alloc += additional;
            }
        }
    }
    // ------- Compute total output (raw) and distribution -------
    let mut total_output_raw = BigUint::zero();
    let mut distribution: Vec<f64> = Vec::with_capacity(size);
    let mut vgas: Vec<u128> = Vec::with_capacity(size);
    for (i, pool) in pools.iter().enumerate() {
        let alloc = allocations[i].clone();
        match pool.protosim.get_amount_out(alloc.clone(), &token_in, &token_out) {
            Ok(result) => {
                // log::info!("Pool {} | Input: {} | Output: {} | Gas: {}", i, alloc, result.amount, result.gas);
                let output = result.amount;
                let gas = result.gas.to_string().parse::<u128>().unwrap_or_default();
                vgas.push(gas);
                // log::info!("Pool {} | Input: {} | Output: {} | Gas: {}", i, alloc, output, gas);
                total_output_raw += &output;
                let percent = (alloc.to_string().parse::<f64>().unwrap() * 100.0f64) / inputpow.to_string().parse::<f64>().unwrap(); // Distribution percentage (integer percentage).
                distribution.push((percent * 100.).round() / 100.); // Round to 3 decimal places.
            }
            Err(_e) => {
                // Often due to 'No Liquidity' error.
                // log::error!("get_amount_out on pool #{} at {} failed: {}", i, pool.component.id, e);
                distribution.push(0.);
                vgas.push(0);
                // ToDo: Re-Allocate the failed pool to the others
                // reallocation.push(allocations[i].clone());
            }
        }
    }
    let token_in_multiplier = 10f64.powi(token_in.decimals as i32);
    let token_out_multiplier = 10f64.powi(token_out.decimals as i32);
    let output = total_output_raw.to_string().parse::<f64>().unwrap() / token_out_multiplier; // Convert raw output to human–readable (divide by token_out multiplier).
    let ratio = ((total_output_raw.to_string().parse::<f64>().unwrap() * token_in_multiplier) / inputpow.to_string().parse::<f64>().unwrap()) / token_out_multiplier; // Compute unit price (as integer ratio of raw outputs times token multipliers).
    TradeResult {
        input: input.to_string().parse().unwrap(),
        output: output.to_string().parse().unwrap(),
        distribution: distribution.clone(),
        gas_costs: vgas.clone(),
        ratio: ratio.to_string().parse().unwrap(),
    }
}
