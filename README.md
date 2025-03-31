
# Tycho TAP-2

@authors 0xMerso & FBerger
@contact Twitter @0xMerso
@protocol Tycho
@program Tycho Orderbook 
@link [Tycho TAP-2](https://github.com/propeller-heads/tycho-x/blob/main/TAP-2.md)
@date March 2025

Tycho Orderbook is a Rust crate that transposes onchain liquidity from AMMs into a familiar orderbook format.
It simulates trades to derive an incremental supply curve and reconstructs liquidity as discrete limit orders.
This unified approach aggregates fragmented liquidity from multiple pools, enabling traders to visualize depth, predict prices, and execute trades directly on-chain while accounting for gas costs.
This makes on-chain liquidity accessible and efficient for trading strategies on markets with classic orderbook.

Usage is simple:
1. Integrate the crate
2. Subscribe to Tycho state updates
3. Retrieve order book data and connect your traditional orderbook trading strategy to it

Quickstart guides and a frontend visualization tool help users get started quickly.

We're looking for contributors, so don't hesitate to open issues, do PR and contact us at @0xMerso.
