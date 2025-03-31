RED='\033[0;31m'
NC='\033[0m'
NETWORK="$1"

# --- Usage ---
# Requires Rust and Cargo to be installed.
# sh examples/local.demo.sh ethereum

function start() {
    trap '' SIGINT
    echo "Building ..."
    export NETWORK=$NETWORK
    cargo build --bin ORDERBOOK -q 2>/dev/null
    echo "Build successful. Executing..."
    (
        trap - SIGINT
        cargo run --bin ORDERBOOK -q # 2>/dev/null
    )
    echo "Program has finished or was interrupted. Continuing with the rest of the shell script ..."
    status+=($?)
    if [ $status -ne 0 ]; then
        echo "Error: $status on program ${RED}${program}${NC}"
        exit 1
    fi
}

start
