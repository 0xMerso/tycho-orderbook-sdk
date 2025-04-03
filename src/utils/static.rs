use crate::types::Network;

/// Static data for the networks
/// https://docs.propellerheads.xyz/tycho/for-solvers/execution/contract-addresses
pub fn networks() -> Vec<Network> {
    vec![
        Network {
            chainid: 1,
            name: "ethereum".to_string(),
            eth: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(),
            rpc: "https://eth.llamarpc.com".to_string(),
            exp: "https://etherscan.io/".to_string(),
            tycho: "tycho-beta.propellerheads.xyz".to_string(),
            permit2: "0x000000000022D473030F116dDEE9F6B43aC78BA3".to_string(),
            router: "0x0178f471f219737c51d6005556d2f44de011a08a".to_string(),
            port: 42001,
        },
        Network {
            chainid: 8453,
            name: "base".to_string(),
            eth: "0x4200000000000000000000000000000000000006".to_string(),
            rpc: "https://mainnet.base.org".to_string(),
            exp: "https://basescan.io/".to_string(),
            tycho: "tycho-base-beta.propellerheads.xyz".to_string(),
            permit2: "0x000000000022D473030F116dDEE9F6B43aC78BA3".to_string(),
            router: "0xC2C23b0199525DE070D126860133dc3badaD2EEb".to_string(),
            port: 42003,
        },
        Network {
            chainid: 130,
            name: "unichain".to_string(),
            eth: "0x4200000000000000000000000000000000000006".to_string(),
            rpc: "https://unichain.drpc.org".to_string(),
            exp: "https://unichain.blockscout.com/".to_string(),
            tycho: "tycho-unichain-beta.propellerheads.xyz".to_string(),
            permit2: "0x000000000022D473030F116dDEE9F6B43aC78BA3".to_string(),
            router: "0x9bdc3be75440dbe563527cb39bb11cfbd1e21b09".to_string(),
            port: 42004,
        },
    ]
}

pub mod maths {

    pub static UNISWAP_Q96: u128 = 1 << 96;
    pub static ONE_HD: f64 = 100.0;
    pub static BPD: f64 = 10_000.0;
    pub static TEN_MILLIONS: f64 = 10_000_000.0;
    pub static ONE_PERCENT_IN_MN: f64 = 10_000.; // 1% in millionths
    pub static MAX_ITERATIONS: u32 = 50; // 50 iteration maximum to optimize allocation
    pub static FRACTION_REALLOC: u32 = 10; // 50 iteration maximum to optimize allocation
    pub static BEST_BID_ASK_ETH_BPS: f64 = 10.; // 10/10_000 = 0.001 ETH = ~2$

    pub mod simu {
        // Config for incrementing amountIn
        pub static COUNT: usize = 50;
        pub static START_MULTIPLIER: f64 = 1.;
        pub static END_MULTIPLIER: f64 = 2_500_000.; // 25% of TEN_MILLIONS

        // pub static MIN_EXP_DELTA: f64 = 15.; Instead of hardcoding it, we use a % of the end multiplier
        pub static MIN_EXP_DELTA_PCT: f64 = 0.0001; // To ensure distance between points
    }
}

pub mod filter {
    pub static REMOVE_TVL_THRESHOLD: f64 = 1.; // 50 iteration maximum to optimize allocation
    pub static ADD_TVL_THRESHOLD: f64 = 500.; // 50 iteration maximum to optimize allocation
    pub static NULL_ADDRESS: &str = "0x0000000000000000000000000000000000000000";
}

pub mod execution {
    pub static EXEC_DEFAULT_SLIPPAGE: f64 = 0.0025;
    pub static APPROVE_FN_SIGNATURE: &str = "approve(address,uint256)";
    pub static DEFAULT_APPROVE_GAS: u64 = 100_000;
}

pub mod endpoints {
    pub static REDIS_LOCAL: &str = "127.0.0.1:7777";
    pub static COINGECKO_ETH_USD: &str = "https://api.coingecko.com/api/v3/simple/price?ids=ethereum&vs_currencies=usd";
}

pub mod data {

    // ! ALL LOWERCASE

    pub mod keys {

        pub mod stream {

            // stream:status:<network> => SyncState
            pub fn tycho(network: String) -> String {
                format!("stream:stream:{}", network.to_lowercase())
            }

            // stream:latest:<network> => u64
            pub fn latest(network: String) -> String {
                format!("stream:latest:{}", network.to_lowercase())
            }

            // stream:latest:<network> => u64
            pub fn updated(network: String) -> String {
                format!("stream:updated:{}", network.to_lowercase())
            }

            // stream:tokens:<network> => array of tokens
            pub fn tokens(network: String) -> String {
                format!("stream:tokens:{}", network.to_lowercase())
            }

            // Get one orderbook via tag
            pub fn orderbook(network: String, tag: String) -> String {
                format!("stream:orderbook:{}:{}", network.to_lowercase(), tag.to_lowercase())
            }

            // stream:component:id => one component
            pub fn component(network: String, id: String) -> String {
                format!("stream:{}:component:{}", network, id.to_lowercase())
            }

            // stream:components
            pub fn components(network: String) -> String {
                format!("stream:components:{}", network.to_lowercase())
            }
        }
    }
}
