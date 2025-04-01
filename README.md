
# tycho-orderbook

**authors:** 0xMerso and FBerger  
**contact:** @0xMerso on X.
**link:** [Tycho TAP-2](https://github.com/propeller-heads/tycho-x/blob/main/TAP-2.md)  
**date:** March 2025  

Tycho Orderbook is a Rust crate that transposes onchain liquidity from AMMs into a familiar orderbook format, thanks to [Tycho](https://docs.propellerheads.xyz/tycho).  
It simulates trades to derive an incremental supply curve and reconstructs liquidity as discrete limit orders.  
This unified approach aggregates fragmented liquidity from multiple pools, enabling traders to visualize depth, predict prices, and execute trades directly on-chain while accounting for gas costs.  
This makes on-chain liquidity accessible and efficient for trading strategies on markets with classic orderbook.  

[Quickstart](https://github.com/0xMerso/tycho-orderbook/blob/main/examples/quickstart.rs) guides and a frontend visualization tool help users get started quickly.

You can run the quickstart, directly with cargo
    
    cargo run --bin quickstart

Or using a custom script (full logs enabled)

    sh examples/quickstart.sh ethereum


We're looking for contributors, so don't hesitate to open issues, do PR and contact us at @0xMerso.
