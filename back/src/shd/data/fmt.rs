use std::collections::HashMap;
use std::str::FromStr;

use alloy::primitives::ruint::aliases::U256;
use chrono::NaiveDateTime;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use tycho_simulation::evm::engine_db::tycho_db::PreCachedDB;
use tycho_simulation::evm::protocol::uniswap_v2::state::UniswapV2State;
use tycho_simulation::evm::protocol::uniswap_v3::state::UniswapV3State;
use tycho_simulation::evm::protocol::uniswap_v4::state::UniswapV4State;
use tycho_simulation::evm::protocol::utils::uniswap::tick_list::{TickInfo, TickList};
use tycho_simulation::evm::protocol::vm::state::EVMPoolState;
use tycho_simulation::evm::tycho_models::Chain;
use tycho_simulation::models::Token;
use tycho_simulation::protocol::models::ProtocolComponent;
use tycho_simulation::tycho_core::Bytes;
use utoipa::ToSchema;

/// @notice Format of the data that will be read/stored in the database
/// By default Tycho object are not srz

// =====================================================================================================================================================================================================
// Tycho Tokens
// =====================================================================================================================================================================================================

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SrzToken {
    #[schema(example = "0xTokenAddress")]
    pub address: String,
    #[schema(example = "6")]
    pub decimals: usize,
    #[schema(example = "ETH")]
    pub symbol: String,
    #[schema(example = "21000")]
    pub gas: String,
}

impl From<Token> for SrzToken {
    fn from(token: Token) -> Self {
        SrzToken {
            address: token.address.to_string(),
            decimals: token.decimals,
            symbol: token.symbol,
            gas: token.gas.to_string(), // Convert BigUint to String
        }
    }
}

impl From<SrzToken> for Token {
    fn from(serialized: SrzToken) -> Self {
        Token {
            address: Bytes::from_str(serialized.address.to_lowercase().as_str()).unwrap(),
            decimals: serialized.decimals,
            symbol: serialized.symbol,
            gas: BigUint::parse_bytes(serialized.gas.as_bytes(), 10).expect("Failed to parse BigUint"), // Convert String back to BigUint
        }
    }
}

// =====================================================================================================================================================================================================
// Tycho Compoment: Convert a ProtocolComponent to a serialized version (and more readable)
// =====================================================================================================================================================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SrzProtocolComponent {
    #[schema(example = "0xTokenAddress")]
    pub address: String,
    #[schema(example = "0xTokenAddress")]
    pub id: String,
    #[schema(example = "Array of SrzToken")]
    pub tokens: Vec<SrzToken>,
    #[schema(example = "uniswap_v4")]
    pub protocol_system: String,
    #[schema(example = "uniswap_v4_pool")]
    pub protocol_type_name: String,
    // pub chain: Chain,
    #[schema(example = "Array of contract ids")]
    pub contract_ids: Vec<String>,
    #[schema(example = "Vector <String, String> of static attributes (fee, tick_spacing, etc)")]
    pub static_attributes: Vec<(String, String)>,
    #[schema(example = "0xHash")]
    pub creation_tx: String,
    // pub created_at: NaiveDateTime,
}

impl SrzProtocolComponent {
    pub fn contains(&self, token: &str) -> bool {
        self.tokens.iter().any(|t| t.symbol.eq_ignore_ascii_case(token))
    }
}

impl From<ProtocolComponent> for SrzProtocolComponent {
    fn from(pc: ProtocolComponent) -> Self {
        SrzProtocolComponent {
            address: pc.id.to_string().to_lowercase(),
            id: pc.id.to_string().to_lowercase(),
            tokens: pc.tokens.into_iter().map(SrzToken::from).collect(),
            protocol_system: pc.protocol_system,
            protocol_type_name: pc.protocol_type_name,
            // chain: pc.chain,
            contract_ids: pc.contract_ids.into_iter().map(|b| b.to_string()).collect(),
            static_attributes: pc.static_attributes.into_iter().map(|(k, v)| (k, v.to_string())).collect(),
            creation_tx: pc.creation_tx.to_string(),
            // created_at: pc.created_at,
        }
    }
}

// impl From<SrzProtocolComponent> for ProtocolComponent {
//     fn from(serialized: SrzProtocolComponent) -> Self {
//         ProtocolComponent {
//             address: Bytes::from_str(serialized.address.to_lowercase().as_str()).unwrap(),
//             id: Bytes::from_str(serialized.id.to_lowercase().as_str()).unwrap(),
//             tokens: serialized.tokens.into_iter().map(Token::from).collect(),
//             protocol_system: serialized.protocol_system,
//             protocol_type_name: serialized.protocol_type_name,
//             // chain: serialized.chain,
//             contract_ids: serialized.contract_ids.into_iter().map(|s| Bytes::from(s.into_bytes())).collect(),                     // !
//             static_attributes: serialized.static_attributes.into_iter().map(|(k, v)| (k, Bytes::from(v.into_bytes()))).collect(), // !
//             creation_tx: Bytes::from(serialized.creation_tx.into_bytes()),                                                        // !
//             // created_at: serialized.created_at,
//         }
//     }
// }

// =====================================================================================================================================================================================================
// Convert a part of a protocol State to a serialized version (and more readable)
// Not reversible, because the state is not fully serialized (it contains a lot of data)
// =====================================================================================================================================================================================================

// =======> Uniswap v2 <=======

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SrzUniswapV2State {
    pub id: String,
    pub reserve0: u128,
    pub reserve1: u128,
}

impl From<(UniswapV2State, String)> for SrzUniswapV2State {
    fn from((state, id): (UniswapV2State, String)) -> Self {
        SrzUniswapV2State {
            id,
            reserve0: state.reserve0.to_string().parse().expect("UniswapV2State: Failed to parse u128"),
            reserve1: state.reserve1.to_string().parse().expect("UniswapV2State: Failed to parse u128"),
        }
    }
}

// =======> Uniswap v3 <=======

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SrzUniswapV3State {
    pub id: String,
    pub liquidity: u128,
    pub sqrt_price: U256,
    pub fee: i32,
    pub tick: i32,
    pub ticks: SrzTickList,
}

impl From<(UniswapV3State, String)> for SrzUniswapV3State {
    fn from((state, id): (UniswapV3State, String)) -> Self {
        SrzUniswapV3State {
            id,
            liquidity: state.liquidity.to_string().parse().expect("UniswapV3State: Failed to parse u128"),
            sqrt_price: state.sqrt_price,
            fee: state.fee as i32,
            tick: state.tick,
            ticks: SrzTickList::from(state.ticks), // ! TODO: sort by index
        }
    }
}

// =======> Uniswap v4 <========

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SrzUniswapV4State {
    pub id: String,
    pub liquidity: u128,
    pub sqrt_price: U256,
    pub fees: SrzUniswapV4Fees,
    pub tick: i32,
    pub ticks: SrzTickList,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SrzUniswapV4Fees {
    pub zero_for_one: u32, // Protocol fees in the zero for one direction
    pub one_for_zero: u32, // Protocol fees in the one for zero direction
    pub lp_fee: u32,       // Liquidity providers fees
}

impl From<(UniswapV4State, String)> for SrzUniswapV4State {
    fn from((state, id): (UniswapV4State, String)) -> Self {
        SrzUniswapV4State {
            id,
            liquidity: state.liquidity.to_string().parse().expect("UniswapV4State: Failed to parse u128"),
            sqrt_price: state.sqrt_price,
            fees: SrzUniswapV4Fees {
                zero_for_one: state.fees.zero_for_one,
                one_for_zero: state.fees.one_for_zero,
                lp_fee: state.fees.lp_fee,
            },
            tick: state.tick,
            ticks: SrzTickList::from(state.ticks), // ! TODO: sort by index
        }
    }
}

// =======> Uniswap v3/v4 <=======

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SrzTickList {
    pub tick_spacing: u16,
    pub ticks: Vec<SrzTickInfo>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct SrzTickInfo {
    pub index: i32,
    pub net_liquidity: i128,
    pub sqrt_price: U256, // ? Is it sqrt_price of tick_index or tick_index + tick_spacing ?
}

impl From<TickInfo> for SrzTickInfo {
    fn from(t: TickInfo) -> Self {
        SrzTickInfo {
            index: t.index,
            net_liquidity: t.net_liquidity.to_string().parse().expect("TickInfo: Failed to parse i128"),
            sqrt_price: t.sqrt_price,
        }
    }
}

impl From<TickList> for SrzTickList {
    fn from(ticks: TickList) -> Self {
        SrzTickList {
            tick_spacing: ticks.tick_spacing,
            ticks: ticks.ticks.into_iter().map(SrzTickInfo::from).collect(),
        }
    }
}

// =======> EVMPoolState <========

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SrzEVMPoolState {
    pub id: String,
    pub tokens: Vec<String>,
    pub block: u64,
    pub balances: HashMap<String, U256>,
    // pub capabilities: HashSet<U256>,
}

// From EVMPoolState to SrzEVMPoolState done manually.

impl From<(EVMPoolState<PreCachedDB>, String)> for SrzEVMPoolState {
    fn from((state, id): (EVMPoolState<PreCachedDB>, String)) -> Self {
        SrzEVMPoolState {
            id,
            tokens: state.tokens.iter().map(|t| t.to_string().to_lowercase()).collect(),
            block: state.block.number,
            balances: state.balances.iter().map(|(k, v)| (k.to_string().to_lowercase(), *v)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {

    use std::str::FromStr;

    use super::*;
    use num_bigint::BigUint;

    #[test]
    fn test_token_to_srztoken_conversion() {
        let token = Token {
            address: Bytes::from_str("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2").unwrap(), // Example address
            decimals: 18,
            symbol: "ETH".to_string(),
            gas: BigUint::from(1000u32), // Example gas value
        };

        let srz_token: SrzToken = token.clone().into();
        assert_eq!(srz_token.address, "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
        assert_eq!(srz_token.decimals, 18);
        assert_eq!(srz_token.symbol, "ETH");
        assert_eq!(srz_token.gas, "1000"); // Ensure BigUint is properly converted to string
    }

    #[test]
    fn test_srztoken_to_token_conversion() {
        let srz_token = SrzToken {
            address: "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2".to_string(),
            decimals: 18,
            symbol: "ETH".to_string(),
            gas: "1000".to_string(), // Stored as a string
        };

        let token: Token = srz_token.clone().into();

        assert_eq!(token.address, Bytes::from_str("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2").unwrap());
        assert_eq!(token.decimals, 18);
        assert_eq!(token.symbol, "ETH");
        assert_eq!(token.gas, BigUint::from(1000u32)); // Ensure string converts back to BigUint
    }

    #[test]
    fn test_round_trip_conversion() {
        let original_token = Token {
            address: Bytes::from_str("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2").unwrap(), // Example address
            decimals: 8,
            symbol: "BTC".to_string(),
            gas: BigUint::from(5000u32),
        };
        let srz_token: SrzToken = original_token.clone().into();
        let converted_token: Token = srz_token.into();
        assert_eq!(original_token, converted_token, "Round trip conversion failed");
    }
}
