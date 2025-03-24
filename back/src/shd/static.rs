pub mod maths {

    pub static UNISWAP_Q96: u128 = 1 << 96;
    pub static BPD: f64 = 10_000.0;
    pub static TEN_MILLIONTH: f64 = 10_000_000.0;
    pub static ONE_PERCENT_IN_MN: f64 = 10_000.; // 1% in millionths
    pub static MAX_ITERATIONS: u32 = 50; // 50 iteration maximum to optimize allocation
    pub static FRACTION_REALLOC: u32 = 10; // 50 iteration maximum to optimize allocation

    pub mod simu {
        // Config for incrementing amountIn
        pub static COUNT: usize = 25;
        pub static START_MULTIPLIER: f64 = 1.;
        pub static END_MULTIPLIER: f64 = 500_000.; // 25% is enough
        pub static MIN_EXP_DELTA: f64 = 15.;
    }
}

pub mod execution {
    pub static EXEC_DEFAULT_SLIPPAGE: f64 = 0.0025;
    pub static APPROVE_FN_SIGNATURE: &str = "approve(address,uint256)";
    pub static DEFAULT_APPROVE_GAS: u128 = 100_000;
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
            pub fn status(network: String) -> String {
                format!("stream:status:{}", network.to_lowercase())
            }

            // stream:status2:<network> => SyncState => Used to wait for Stream to fully sync (balances)
            pub fn stream2(network: String) -> String {
                format!("stream2:status:{}", network.to_lowercase())
            }

            // stream:latest:<network> => u64
            pub fn latest(network: String) -> String {
                format!("stream:latest:{}", network.to_lowercase())
            }

            // stream:latest:<network> => u64
            pub fn updatedcps(network: String) -> String {
                format!("stream:updatedcps:{}", network.to_lowercase())
            }

            // stream:tokens:<network> => array of tokens
            pub fn tokens(network: String) -> String {
                format!("stream:tokens:{}", network.to_lowercase())
            }

            // stream:pairs:<network> => array of pairs
            pub fn pairs(network: String) -> String {
                format!("stream:pairs:{}", network.to_lowercase())
            }

            // stream:component:id => one component
            pub fn component(network: String, id: String) -> String {
                format!("stream:{}:component:{}", network, id.to_lowercase())
            }

            // stream:state:id => one state
            pub fn state(network: String, id: String) -> String {
                format!("stream:{}:state:{}", network, id.to_lowercase())
            }

            // stream:component:<id> => Component (serialized)
            pub fn components(network: String) -> String {
                format!("stream:components:{}", network.to_lowercase())
            }

            // stream:component:<id> => ProtocolState (serialized)
            pub fn states(network: String) -> String {
                format!("stream:state:{}", network.to_lowercase())
            }
        }
    }
}
