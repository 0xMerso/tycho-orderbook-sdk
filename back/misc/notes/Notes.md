# Notes

Tycho is an open-source interface to on-chain liquidity. Tycho
- Indexes DEX protocol state for you with low latency,
- Simulates swaps extremely fast with one interface for all DEXs, and  
- Executes swaps on-chain

### Links

- https://www.erc7815.org/
- https://github.com/propeller-heads/tycho-simulation
- https://docs.propellerheads.xyz/tycho/for-solvers/simulation
- https://docs.propellerheads.xyz/tycho/for-solvers/supported-protocols
- https://www.propellerheads.xyz/blog/amm-liquidity-as-an-orderbook
- https://www.rareskills.io/post/uniswap-v3-concentrated-liquidity
- https://www.lvr.wtf/category
- https://www.rareskills.io/post/uniswap-v3-ticks
- https://www.highcharts.com/demo/stock/depth-chart
- https://grafana.propellerheads.xyz/public-dashboards/518dd877a470434383caf9fc5845652e?orgId=1&refresh=5s
- https://docs.google.com/spreadsheets/d/1vDl57BthpeJ9WDqmCVXFdLLvmsm-_SwhSjVzobXJZ04/edit?gid=0#gid=0
  
### Keep in mind

There are two types of implementations:
- Native protocols are ported to Rust – for faster simulation.
- VM protocols execute the VM bytecode locally – which is easier to integrate but has slower simulation times than a native implementation.


Lower tick spacing allows finer price management, but leads to more on-chain transactions fees
Higher tick spacing means that liquidity is used in wider intervals, reducing the cost of gas.
A highly volatile asset requires a wider tick spacing (e.g. 200) to avoid too frequent liquidity adjustments.
A stablecoin requires a lower tick spacing (e.g. 1) to allow more precise arbitrage.
Tick spacing = 1 for 1 bps pools, 10 for 5 bps, 60 for 30 bps and 200 for 100 bps
The tick closest to the current sqrt price represents the active liquidity range.
When price moves across a tick, the liquidity at that tick is either added or removed from the active liquidity.
Active liquidity can't be negative.

Ticks: Divide the price continuum; each tick i corresponds to \sqrt{P(i)} = 1.0001^{\frac{i}{2}}.
	•	Active Liquidity: The liquidity currently active at the current tick.
	•	Net Liquidity Deltas: Changes stored at each tick that adjust the active liquidity when the price crosses that tick.
	•	Cumulative Liquidity: Calculated by adding/subtracting net deltas from the active liquidity as the price moves.
	•	Token Amount Conversion:
	•	Below range: All liquidity is in token0.
	•	Above range: All liquidity is in token1.
	•	Within range: Liquidity splits between token0 and token1, with the split determined by the current \sqrt{P} relative to the boundaries.
	•	Locked Liquidity: A liquidity provider’s deposited amounts determine an L that is the minimum of two liquidity calculations (one for each token), ensuring that the position is balanced.
	•	Edge Cases:
If the current \sqrt{P} equals a boundary, one token’s amount will be nearly zero.

This overview captures the core ideas behind ticks, liquidity adjustments, and how token amounts are derived from liquidity in Uniswap V3’s concentrated liquidity model.

The net liquidity for all ticks combined (across the entire range) cancels out to zero because each liquidity provider’s position has an addition at the lower tick and an equal removal at the upper tick.

You can view tick/fees stuff when creating a pool.
[Here](https://app.uniswap.org/positions/create/v3?currencyA=0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48&currencyB=0xdAC17F958D2ee523a2206206994597C13D831ec7&chain=ethereum) for example

The liquidityNet of a tick is an aggregate measure of all the liquidity referencing the tick across different liquidity positions where the tick could simultaneously be the lower tick of one position and the upper tick of another position. It's a signed 128-bit integer(i.e. int128) reflecting the net effect of all changes in liquidity at that particular tick

he ProtocolComponent struct is the only way to specify a swap route, so every Substream integration must use it. However, a "component" doesn’t necessarily mean a pool - it can be anything that allows users to swap one token for another.  

The allowed tick indexes range from -887,272 to 887,272
If the tick index is negative, this corresponds to a price less than 1, since  will be less than 1 if is negative.
If then the price is 1 (meaning the assets have equal value) because any value raised to 0 is 1.
If is greater than or equal to 1, then the price will be greater than one.
The value 1.0001 was chosen because “This has the desirable property of each tick being a .01% (1 basis point) price movement away from each of its neighboring ticks.”
The “current tick” is the current price rounded down to the nearest tick. If the price increases and crosses a tick, then the tick that was just crossed becomes the current tick. “Crossed” doesn’t require that the priced “passed over” the tick.
If the price stops on the tick, the tick is considered crossed.

How decimals affect price
The tick can be negative due to the difference in decimal places, as ETH has 18 decimals while USDC has only 6 decimals.
Assuming ETH is worth $1000, the smallest unit of ETH is worth (or ), while the smallest unit of USDC is worth
So, although we assume that 1 ETH is “worth” more than 1 USDC, considering its smallest unit, 1 smallest unit of USDC is worth more than 1 smallest unit of ETH.
