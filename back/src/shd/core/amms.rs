

use crate::shd::{
    r#static::maths::BPD,
    types::AmmType,
};

/**
 * Convert Tycho fee attributes to basis point fee
 */

/// Converts a native fee (as a hex string) into a byte vector representing fee in basis points.
/// The conversion depends on the protocol type:
/// - uniswap_v2_pool: fee is already in basis points (e.g., "0x1e" → 30)
/// - uniswap_v3_pool or uniswap_v4_pool: fee is stored on a 1e6 scale (so 3000 → 30 bps, i.e. divide by 100)
/// - curve: fee is stored on a pow10 scale (e.g., 4000000 becomes 4 bps, so divide by 1_000_000)
/// - balancer_v2_pool: fee is stored on a pow18 scale (e.g., 1*10^15 becomes 10 bps, so divide by 1e14)
pub fn feebps(protocol: String, _id: String, value: String) -> u128 {
    let fee = value.trim_start_matches("0x");
    let fee = u128::from_str_radix(fee, 16).unwrap_or(0);
    // log::info!("Fee value: {} (from {})", fee, value);
    let fee = match AmmType::from(protocol.as_str()) {
        AmmType::Pancakeswap | AmmType::Sushiswap | AmmType::UniswapV2 => fee, // Already in bps
        AmmType::UniswapV3 | AmmType::UniswapV4 => fee * (BPD as u128) / 1_000_000,
        AmmType::Curve => 4, // Not implemented, assuming 4 bps by default
        AmmType::Balancer => (fee * (BPD as u128)) / 1e18 as u128,
    };
    // log::info!("Proto: {} | ID: {} | Fee in bps: {} | Initial: {}", protocol, _id, fee, value);
    fee
    // "uniswap_v2_pool" => fee_value,                           // already in bps
    // "uniswap_v3_pool" | "uniswap_v4_pool" => fee_value / 100, // 1e6 scale → bps conversion
    // "curve" => fee_value / 1_000_000,                         // pow10 scale → bps conversion
    // "balancer_v2_pool" => fee_value / 100_000_000_000_000,    // pow18 scale → bps conversion
    // _ => fee_value,                                           // default: no conversion applied
}
