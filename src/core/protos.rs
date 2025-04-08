use crate::{types::AmmType, utils::r#static::maths::BPD};

/// Converts a native fee (as a hex string) into a byte vector representing fee in basis points.
/// The conversion depends on the protocol type:
/// - uniswap_v2_pool: fee is already in basis points (e.g., "0x1e" → 30)
/// - uniswap_v3_pool or uniswap_v4_pool: fee is stored on a 1e6 scale (so 3000 → 30 bps, i.e. divide by 100)
/// - curve: fee is stored on a pow10 scale (e.g., 4000000 becomes 4 bps, so divide by 1_000_000)
/// - balancer_v2_pool: fee is stored on a pow18 scale (e.g., 1*10^15 becomes 10 bps, so divide by 1e14)
pub fn amm_fee_to_bps(protocol: String, _id: String, value: String) -> u128 {
    let fee = value.trim_start_matches("0x");
    let fee = u128::from_str_radix(fee, 16).unwrap_or(0);
    let fee = match AmmType::from(protocol.as_str()) {
        AmmType::PancakeswapV2 | AmmType::Sushiswap | AmmType::UniswapV2 => fee, // Already in bps
        AmmType::PancakeswapV3 | AmmType::UniswapV3 | AmmType::UniswapV4 => fee * (BPD as u128) / 1_000_000,
        AmmType::Curve => 4,   // Not implemented, assuming 4 bps by default
        AmmType::EkuboV2 => 0, // Not implemented, assuming 0 bps by default
        AmmType::Balancer => (fee * (BPD as u128)) / 1e18 as u128,
    };
    fee
}
