/**
 * Component Pool Balances
 * ! Not viable for now as we cannot compute balance of Uniswap v4 pools
 * Fetch the total avalable liquidity balance of component's tokens, depending the pool type
 * Divided by 10^decimals to get the real balance
 */
pub async fn get_pool_balances(network: Network, provider: &RootProvider<Http<Client>>, cp: SrzProtocolComponent) -> Vec<f64> {
    log::info!("Fetching balances for '{}' => id: {}", cp.protocol_type_name, cp.id);
    let client = Arc::new(provider);
    let mut balances = cp.tokens.iter().map(|_t| 0.).collect::<Vec<f64>>();
    match cp.protocol_type_name.as_str() {
        "uniswap_v2_pool" | "uniswap_v3_pool" | "curve" => {
            for (i, t) in cp.tokens.iter().enumerate() {
                let contract = IERC20::new(t.address.parse().unwrap(), client.clone());
                let balance = contract.balanceOf(cp.address.parse().unwrap()).call().await.unwrap().balance.to_string().parse::<u128>().unwrap();
                let balance = balance as f64 / 10u128.pow(t.decimals as u32) as f64;
                log::info!(" uniswap_v2_pool // uniswap_v3_pool // curve => {} balance: {:.2}", t.symbol, balance);
                balances[i] = balance;
            }
        }
        "uniswap_v4_pool" => {
            log::error!("🔺 Uniswap v4 pool balance not supported");
            // Due to efficiency reasons, the exact token balance changes are not recorded anywhere on the blockchain during a swap
            // Instead, the system keeps track of just the liquidity variable, the current price, tick, and tick range.
            // https://docs.uniswap.org/contracts/v4/deployments
            // Get u4 pools balances
            // v4 => (https://github.com/Uniswap/v4-periphery/blob/main/src/interfaces/IStateView.sol)
            // https://etherscan.io/address/0x7ffe42c4a5deea5b0fec41c94c136cf115597227#readContract
            // PooL ID: 0xe018f09af38956affdfeab72c2cefbcd4e6fee44d09df7525ec9dba3e51356a5
            // https://soliditydeveloper.com/uniswap4
            // https://github.com/Uniswap/v4-core/blob/main/test/utils/LiquidityAmounts.sol

            // Solution => Subgraph: https://thegraph.com/explorer/subgraphs/DiYPVdygkfjDWhbxGSqAQxwBKmfKnkWQojqeM2rkLb3G?view=About&chain=arbitrum-one
        }
        "balancer_v2_pool" => {
            // v2: https://docs-v2.balancer.fi/reference/contracts/deployment-addresses/arbitrum.html
            // Example: https://etherscan.io/address/0xba12222222228d8ba445958a75a0704d566bf2c8#readContract
            // - 0x96646936b91d6b9d7d0c47c496afbf3d6ec7b6f8000200000000000000000019
            // v3: https://docs.balancer.fi/developer-reference/contracts/deployment-addresses/arbitrum.html
            let contract = IBalancer2Vault::new(network.balancer.parse().unwrap(), client.clone());
            match contract.getPoolTokens(cp.id.to_string().parse().unwrap()).call().await {
                Ok(res) => {
                    for (i, t) in cp.tokens.iter().enumerate() {
                        let btk = res.tokens[i].to_string();
                        if btk.to_lowercase() == t.address.to_lowercase() {
                            let balance = res.balances[i].to_string().parse::<u128>().unwrap();
                            let balance = balance as f64 / 10u128.pow(t.decimals as u32) as f64;
                            balances[i] = balance;
                            log::info!(" balancer_v2_pool => {} balance: {:.2}", t.symbol, balance);
                        } else {
                            log::error!("Balancer pool token address mismatch: {} != {}", btk, t.address);
                            balances[i] = 0.;
                        }
                    }
                }
                Err(e) => {
                    log::error!("Error getting balancer pool tokens: {} on balancer_v2_pool", e.to_string());
                }
            }
        }
        _ => {
            log::info!("Unknown protocol type: {}", cp.protocol_type_name);
        }
    }
    balances
}
