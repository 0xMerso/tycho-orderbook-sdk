#!/bin/bash

RED='\033[0;31m'
NC='\033[0m'
NETWORK="$1"

# --- Usage ---
# Requires Rust and Cargo to be installed.
# sh examples/quickstart.sh ethereum|base
# By default, it would swap 0.001 worth of WETH in USDC token (< 2$) (base to quote)
# You just need to provide the private key with 0.001 WETH in the wallet, Erc20 wrapped ETH.
# You can also edit the quickstart.rs file to change the .env file to the one gitignored (.env.quickstart) and put your private key there.

# The .env.quickstart file is expected to be in examples folder, NOT in the root folder.

function start() {
    trap '' SIGINT
    export NETWORK=$NETWORK
    echo "Building Quickstart for {$NETWORK} (might take a few minutes the first time) ..."
    cargo build --bin quickstart -q 2>/dev/null
    echo "Build successful. Executing..."
    (
        trap - SIGINT
        export RUST_LOG="off,tycho_orderbook=trace,quickstart=trace"
        # export RUST_LOG="off,tycho_orderbook=trace,quickstart=trace,tycho_execution=trace"
        cargo run --bin quickstart -q # 2>/dev/null
    )
    echo "Program has finished or was interrupted. Continuing with the rest of the shell script ..."
    status+=($?)
    if [ $status -ne 0 ]; then
        echo "Error: $status on program ${RED}${program}${NC}"
        exit 1
    fi
}

start

# --- Or just run the binary directly ---
# cargo run --bin quickstart
