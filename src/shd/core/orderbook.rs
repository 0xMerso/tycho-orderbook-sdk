use tycho_simulation::{models::Token, protocol::state::ProtocolSim};

use crate::shd::{
    data::fmt::SrzToken,
    types::{Network, PairQuery, PairSimulatedOrderbook, ProtoTychoState, TradeResult},
};

/// @notice Reading 'state' from Redis DB while using TychoStreamState state and functions to compute/simulate might create a inconsistency
pub async fn build(network: Network, datapools: Vec<ProtoTychoState>, tokens: Vec<SrzToken>, query: PairQuery) -> PairSimulatedOrderbook {
    log::info!("Got {} pools to compute for pair: '{}'", datapools.len(), query.tag);
    let mut pools = Vec::new();
    let mut prices0to1 = vec![];
    let mut prices1to0 = vec![];
    // let mut balance0 = vec![];
    // let mut balance1 = vec![];
    for pdata in datapools.clone() {
        log::info!("Preparing pool: {} | Type: {}", pdata.component.id, pdata.component.protocol_type_name);
        if pdata.component.protocol_type_name.to_lowercase() == "uniswap_v4_pool" || pdata.component.protocol_type_name.to_lowercase() == "balancer_v2_pool" {
            log::info!("Skipping pool {} because it's {}", pdata.component.id, pdata.component.protocol_type_name.to_lowercase());
            continue;
        }
        pools.push(pdata.clone());
        let srzt0 = tokens[0].clone();
        let srzt1 = tokens[1].clone();
        let t0 = Token::from(srzt0.clone());
        dbg!(t0.clone());
        let t1 = Token::from(srzt1.clone());
        dbg!(t1.clone());
        let (base, quote) = if query.z0to1 { (t0, t1) } else { (t1, t0) };
        let proto = pdata.protosim.clone();
        let price0to1 = proto.spot_price(&base, &quote).unwrap_or_default();
        let price1to0 = proto.spot_price(&base, &quote).unwrap_or_default();
        prices0to1.push(proto.spot_price(&base, &quote).unwrap_or_default());
        prices1to0.push(proto.spot_price(&base, &quote).unwrap_or_default());
        // let poolb0 = fetchbal(&provider, srzt0.address.to_string(), pdata.component.id.clone()).await;
        // let poolb1 = fetchbal(&provider, srzt1.address.to_string(), pdata.component.id.clone()).await;
        log::info!("Spot price for {}-{} => price0to1 = {} and price1to0 = {}", base.symbol, quote.symbol, price0to1, price1to0);
        log::info!("\n");
    }
    let avgp0to1 = prices0to1.iter().sum::<f64>() / prices0to1.len() as f64;
    let avgp1to0 = prices0to1.iter().sum::<f64>() / prices0to1.len() as f64;
    log::info!("Average price 0to1: {} | Average price 1to0: {}", avgp0to1, avgp1to0);
    optimization(network.clone(), pools.clone(), tokens, query).await
    // PairSimulatedOrderbook {
    //     from: tokens[0].clone(),
    //     to: tokens[1].clone(),
    //     trades: vec![],
    //     pools: pools.clone(),
    // }
}

use num_bigint::BigUint;
use num_traits::{One, Pow, Zero};
use std::time::Instant;

/**
 * Generates a set of test amounts for ETH.
 */
fn generate_eth_steps() -> Vec<BigUint> {
    let mut steps = Vec::new();
    // First segment: 1 to 100 by 5.
    let step1 = BigUint::from(1u32);
    let mut x = BigUint::from(1u32);
    while x < BigUint::from(100u32) {
        steps.push(x.clone());
        x = &x + &step1;
    }
    steps.push(BigUint::from(100u32));
    // Second segment: 100 to 1000 by 25.
    let step2 = BigUint::from(25u32);
    let mut x = BigUint::from(100u32);
    while x < BigUint::from(1000u32) {
        steps.push(x.clone());
        x = &x + &step2;
    }
    steps.push(BigUint::from(1000u32));
    // Third segment: 1000 to 25000 by 500.
    let step3 = BigUint::from(500u32);
    let mut x = BigUint::from(1000u32);
    while x < BigUint::from(25000u32) {
        steps.push(x.clone());
        x = &x + &step3;
    }
    steps.push(BigUint::from(10000u32));
    steps
}

/**
 * Generates a set of test amounts for USDC.
 * By multiplying each ETH step by 2000 (since 1 ETH = 2000 USDC).
 */
fn generate_usdc_steps() -> Vec<BigUint> {
    generate_eth_steps().into_iter().map(|eth_amount| eth_amount * BigUint::from(2000u32)).collect()
}

/**
 * A very simple gradient-based optimizer that uses fixed iterations (100 max) and
 * moves a fixed fraction (10%) of the allocation from the pool with the lowest marginal
 * return to the one with the highest.
 * All arithmetic is done with BigUint.
 */
pub fn optimizer(
    total_input: BigUint, // human–readable input (e.g. 100 meaning 100 ETH)
    pools: &Vec<ProtoTychoState>,
    token_in: SrzToken,
    token_out: SrzToken,
) -> TradeResult {
    // Convert tokens to simulation tokens.
    let sim_token_in = Token::from(token_in.clone());
    let sim_token_out = Token::from(token_out.clone());
    let token_in_multiplier = 10f64.powi(token_in.decimals as i32); // get_multiplier_bg(&token_in);
    let token_out_multiplier = 10f64.powi(token_out.decimals as i32); // get_multiplier_bg(&token_out);
                                                                      // log::info!("Token in multiplier: {}, Token out multiplier: {}", token_in_multiplier, token_out_multiplier);
    let token_in_multiplier_bg = BigUint::from(10u32).pow(token_in.decimals);
    let inputraw = &total_input * &token_in_multiplier_bg;
    let size = pools.len();
    let sizebg = BigUint::from(size as u32);
    let mut allocations: Vec<BigUint> = vec![&inputraw / &sizebg; size]; // Which is naive I guess

    // @notice epsilon is key here. It tells us the marginal benefit of giving a little more to that pool. The smaller epsilon is, the more accurately we capture that local behavior
    let epsilon = &inputraw / BigUint::from(10_000u32); // Choose a fixed epsilon for finite difference. May 1e9 is better, IDK.
    let max_iterations = 100u32; // We'll run a maximum of 100 iterations.
    let tolerance = BigUint::zero(); // Tolerance: if the difference between max and min marginal is zero.
    for iter in 0..max_iterations {
        // Compute marginal returns for each pool as: f(x+epsilon) - f(x).
        let mut marginals: Vec<BigUint> = Vec::with_capacity(size);
        // If the difference between the best and worst marginal return becomes zero (or falls below a tiny tolerance),
        // then the algorithm stops early because it has “converged” on an allocation where no pool can provide a better extra return than any other.
        for (i, pool) in pools.iter().enumerate() {
            let current_alloc = allocations[i].clone();
            let got = pool.protosim.get_amount_out(current_alloc.clone(), &sim_token_in, &sim_token_out).unwrap().amount;
            let espgot = pool.protosim.get_amount_out(&current_alloc + &epsilon, &sim_token_in, &sim_token_out).unwrap().amount;
            let marginal = if espgot > got { &espgot - &got } else { BigUint::zero() };
            marginals.push(marginal);
        }
        // Identify pools with maximum and minimum marginals.
        let (max, max_marginal) = marginals.iter().enumerate().max_by(|a, b| a.1.cmp(b.1)).unwrap();
        let (mini, min_marginal) = marginals.iter().enumerate().min_by(|a, b| a.1.cmp(b.1)).unwrap();
        // If difference is zero (or below tolerance), stop.
        if max_marginal.clone() - min_marginal.clone() <= tolerance {
            log::info!("Converged after {} iterations", iter);
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

    // ------- Compute total output (raw) and distribution -------
    let mut total_output_raw = BigUint::zero();
    let mut distribution: Vec<f64> = Vec::with_capacity(size);
    for (i, pool) in pools.iter().enumerate() {
        let alloc = allocations[i].clone();
        let output = pool.protosim.get_amount_out(alloc.clone(), &sim_token_in, &sim_token_out).unwrap().amount;
        total_output_raw += &output;
        let percent = (alloc.to_string().parse::<f64>().unwrap() * 100.0f64) / inputraw.to_string().parse::<f64>().unwrap(); // Distribution percentage (integer percentage).
        distribution.push(percent);
    }
    let output = total_output_raw.to_string().parse::<f64>().unwrap() / token_out_multiplier; // Convert raw output to human–readable (divide by token_out multiplier).
    let ratio = ((total_output_raw.to_string().parse::<f64>().unwrap() * token_in_multiplier) / inputraw.to_string().parse::<f64>().unwrap()) / token_out_multiplier; // Compute unit price (as integer ratio of raw outputs times token multipliers).
    TradeResult {
        input: total_input.to_string().parse().unwrap(),
        output: output.to_string().parse().unwrap(),
        distribution: distribution.clone(),
        ratio: ratio.to_string().parse().unwrap(),
    }
}

/**
 * Optimizes a trade for a given pair of tokens and a set of pools.
 * The function generates a set of test amounts for ETH and USDC, then runs the optimizer for each amount.
 * The optimizer uses a simple gradient-based approach to move a fixed fraction of the allocation from the pool with the lowest marginal return to the one with the highest.
 */
pub async fn optimization(network: Network, pcsdata: Vec<ProtoTychoState>, tokens: Vec<SrzToken>, query: PairQuery) -> PairSimulatedOrderbook {
    log::info!("Network: {} | Got {} pools to optimize for pair: '{}'", network.name, pcsdata.len(), query.tag);
    let usdc = tokens[0].clone();
    let weth = tokens[1].clone();
    let mut pools = Vec::new();
    for pcdata in pcsdata.iter() {
        log::info!("pcdata: {} | Type: {}", pcdata.component.id, pcdata.component.protocol_type_name);
        pools.push(pcdata.component.clone());
    }
    // Generate test amounts for ETH (human–readable) based on our three segments. Alternatively, for USDC you could use generate_usdc_steps()
    let increments = generate_eth_steps();
    let mut results = Vec::new();
    for amount in increments.iter() {
        let start = Instant::now();
        let result = optimizer(amount.clone(), &pcsdata, weth.clone(), usdc.clone());
        let elapsed = start.elapsed();
        log::info!(
            "Input: {} ETH, Output: {} USDC, Unit Price: {} USDC/ETH, Distribution: {:?}, Time: {:?}",
            result.input,
            result.output,
            result.ratio,
            result.distribution,
            elapsed
        );
        results.push(result);
    }

    let res = PairSimulatedOrderbook {
        from: tokens[0].clone(),
        to: tokens[1].clone(),
        trades: results.clone(),
        pools: pools.clone(),
    };
    let path = format!("misc/data/{}.opti.eth-usdc.orderbook-1to0.json", network.name);
    crate::shd::utils::misc::save1(res.clone(), path.as_str());

    {
        let increments = generate_usdc_steps();
        let mut results = Vec::new();
        for amount in increments.iter() {
            let start = Instant::now();
            let result = optimizer(amount.clone(), &pcsdata, usdc.clone(), weth.clone());
            let elapsed = start.elapsed();
            log::info!(
                "Input: {} USDC, Output: {} WETH, Unit Price: {} ETH/USDC, Distribution: {:?}, Time: {:?}",
                result.input,
                result.output,
                result.ratio,
                result.distribution,
                elapsed
            );
            results.push(result);
        }

        let res = PairSimulatedOrderbook {
            from: tokens[0].clone(),
            to: tokens[1].clone(),
            trades: results.clone(),
            pools: pools.clone(),
        };
        let path = format!("misc/data/{}.opti.eth-usdc.orderbook-0to1.json", network.name);
        crate::shd::utils::misc::save1(res.clone(), path.as_str());
    }

    res
}
