use tycho_simulation::{models::Token, protocol::state::ProtocolSim};

use crate::shd::{
    data::fmt::SrzToken,
    types::{ProtoTychoState, TradeResult},
};
use num_bigint::BigUint;
use num_traits::Zero;

/**
 * A gradient-based optimizer that uses fixed iterations (100 max) and
 * moves a fixed fraction (10%) of the allocation from the pool with the lowest marginal
 * return to the one with the highest.
 * All arithmetic is done with BigUint.
 */
pub fn gradient(
    amount: f64, // human–readable amount (e.g. 100 meaning 100 ETH)
    pools: &Vec<ProtoTychoState>,
    token_in: SrzToken,
    token_out: SrzToken,
    ethusd: f64,            // ETH price in USD
    gas_price: u128,        // Gas price in Gwei
    output_u_ethworth: f64, // How much is one unit of token_out worth in ETH (divided by 1e18). It would be 1. for ETH obviously, or 0.0005 for USD for instance.
) -> TradeResult {
    // Convert tokens to tycho-simulation tokens
    let token_in = Token::from(token_in.clone());
    let token_out = Token::from(token_out.clone());
    let amountpow = amount * 10f64.powi(token_in.decimals as i32).round();

    // let output_u_ethworth = output_u_ethworth * 1e18f64;
    // let output_u_ethworth = BigUint::from(output_u_ethworth as u128);
    let amountpow = BigUint::from(amountpow as u128);
    let size = pools.len();
    let sizebg = BigUint::from(size as u32);
    let mut allocations: Vec<BigUint> = vec![&amountpow / &sizebg; size]; // Which is naive I guess
                                                                          // Reallocate 10% of the allocation from the pool with the lowest marginal.
    let fraction = BigUint::from(10u32);
    // @notice epsilon is key here. It tells us the marginal benefit of giving a little more to that pool. The smaller epsilon is, the more accurately we capture that local behavior
    let epsilon = &amountpow / BigUint::from(10_000u32); // Choose a fixed epsilon for finite difference. May 1e9 is better, IDK.
    let max_iterations = 50u32; // Maximum of iterations.
    let tolerance = BigUint::zero(); // Tolerance: if the difference between max and min marginal is zero.
    for _iter in 0..max_iterations {
        // @dev This is very inneficient
        // Compute marginal returns for each pool as: f(x+epsilon) - f(x).
        let mut marginals: Vec<BigUint> = Vec::with_capacity(size);
        // If the difference between the best and worst marginal return becomes zero (or falls below a tiny tolerance),
        // then the algorithm stops early because it has “converged” on an allocation where no pool can provide a better extra return than any other.
        for (i, pool) in pools.iter().enumerate() {
            let current_alloc = allocations[i].clone();
            // log::info!("Current allocation for pool {}: {} | TokenIn {} TokenOut {}", i, current_alloc, token_in.address, token_out.address.clone());
            let result_got = pool.protosim.get_amount_out(current_alloc.clone(), &token_in, &token_out);

            // Tmp
            // let result_x = pool.protosim.get_amount_out(current_alloc.clone(), &token_in, &token_out).unwrap();
            // let gas = result_x.gas * output_u_ethworth.clone();
            // // ! Need gas_price * gas_amount to get the actual cost.
            // let gas_price = BigUint::from(1_000_000_000u64); // 1 Gwei
            // let gas_cost = gas * gas_price;
            // // log::info!("Gas cost for pool {}: {}", i, gas_cost);
            // let output_equivalent = gas_cost / output_u_ethworth.clone();
            // let output_net = result_x.amount - output_equivalent;

            let (amount_got, gas) = if result_got.is_err() {
                // log::error!("get_amount_out on pool #{} at {} failed", i, pool.component.id);
                (BigUint::ZERO, BigUint::ZERO)
            } else {
                let got = result_got.unwrap();
                (got.amount, got.gas)
            };

            // let gas = gas.to_string().parse::<u128>().unwrap_or_default();
            // let total_gas_cost_in_output_token = ((gas * gas_price) as f64 / 1e18f64) * output_u_ethworth;
            // let total_gas_cost_usd = (gas * gas_price) as f64 * ethusd / 1e18f64;
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
        // => Moving a fixed fraction (10%) of the allocation from the worst-performing pool to the best-performing one
        // Too high a percentage might cause the allocation to swing too quickly, overshooting the optimal balance.
        // Too low a percentage would make convergence very slow.
        let adjusted = &allocations[mini] / &fraction;
        allocations[mini] = &allocations[mini] - &adjusted;
        allocations[max] = &allocations[max] + &adjusted;
        // Once the iterations finish, the optimizer:
        // - Computes the total output by summing the outputs from all pools using the final allocations.
        // - Calculates the percentage of the total input that was allocated to each pool.
        // log::info!("Iteration {}: Pool {} marginal = {} , Pool {} marginal = {}, transfer = {}", iter, max, max_marginal, mini, min_marginal, adjusted);
    }

    // Define minimum allocation per pool as 5% of total input.
    let min_alloc = &amountpow * BigUint::from(5u32) / BigUint::from(100u32);

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
    let mut gas_quantity: Vec<u128> = Vec::with_capacity(size);
    let mut gas_costs_usd: Vec<f64> = Vec::with_capacity(size);
    let mut gas_costs_output: Vec<f64> = Vec::with_capacity(size);
    for (i, pool) in pools.iter().enumerate() {
        let alloc = allocations[i].clone();
        if alloc > BigUint::zero() {
            match pool.protosim.get_amount_out(alloc.clone(), &token_in, &token_out) {
                Ok(result) => {
                    // log::info!("Pool {} | Input: {} | Output: {} | Gas: {}", i, alloc, result.amount, result.gas);
                    let output = result.amount;
                    let gas = result.gas.to_string().parse::<u128>().unwrap_or_default();
                    gas_quantity.push(gas);
                    let gas_cost_usd = (gas * gas_price) as f64 * ethusd / 1e18f64;
                    gas_costs_usd.push(gas_cost_usd);
                    let total_gas_cost_in_eth = ((gas * gas_price) as f64 / 1e18f64) * output_u_ethworth;
                    gas_costs_output.push(total_gas_cost_in_eth);
                    // total_gas_cost_in_eth to output amount
                    let gas_worth_output = total_gas_cost_in_eth / output_u_ethworth; // Unsure, TO CONTINUE.

                    // log::info!("Pool {} | Input: {} | Output: {} | Gas: {}", i, alloc, output, gas);
                    total_output_raw += &output;
                    let percent = (alloc.to_string().parse::<f64>().unwrap() * 100.0f64) / amountpow.to_string().parse::<f64>().unwrap(); // Distribution percentage (integer percentage).
                    distribution.push((percent * 100.).round() / 100.); // Round to 3 decimal places.
                }
                Err(_e) => {
                    // Often due to 'No Liquidity' error.
                    distribution.push(0.);
                    gas_quantity.push(0);
                    gas_costs_usd.push(0.);
                    gas_costs_output.push(0.);
                }
            }
            // log::info!("Pool {} | Input: {}", i, alloc);
        } else {
            // Empty alloc still needs to be accounted for.
            distribution.push(0.);
            gas_quantity.push(0);
            gas_costs_usd.push(0.);
            gas_costs_output.push(0.);
        }
    }
    let token_in_multiplier = 10f64.powi(token_in.decimals as i32);
    let token_out_multiplier = 10f64.powi(token_out.decimals as i32);
    let output = total_output_raw.to_string().parse::<f64>().unwrap() / token_out_multiplier; // Convert raw output to human–readable (divide by token_out multiplier).
    let ratio = ((total_output_raw.to_string().parse::<f64>().unwrap() * token_in_multiplier) / amountpow.to_string().parse::<f64>().unwrap()) / token_out_multiplier; // Compute unit price (as integer ratio of raw outputs times token multipliers).
    TradeResult {
        amount: amount.to_string().parse().unwrap(),
        output: output.to_string().parse().unwrap(),
        distribution: distribution.clone(),
        gas_costs: gas_quantity.clone(),
        gas_costs_usd: gas_costs_usd.clone(),
        gas_costs_output: gas_costs_output.clone(),
        ratio: ratio.to_string().parse().unwrap(),
    }
}
