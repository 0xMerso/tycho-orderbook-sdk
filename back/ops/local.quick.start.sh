RED='\033[0;31m'
NC='\033[0m' # No Color
# NETWORK=ethereum
RPC="https://rpc.payload.de"
NETWORK="$1"

function start() {

    trap '' SIGINT

    # ------------- Redis -------------
    # rm -rf dump.rdb
    # ps -ef | grep redis-server | grep -v grep | awk '{print $2}' | xargs kill 2>/dev/null
    # redis-server --port 7777 --bind 127.0.0.1 2>&1 >/dev/null &
    # # redis-server src/shared/config/redis.conf --bind 127.0.0.1 2>&1 >/dev/null &
    # echo "Redis ready #$(ps -ef | grep redis-server | grep -v grep | awk '{print $2}')"
    # sleep 1
    # ------------- Execute -------------
    echo "Building ..."
    export NETWORK=$NETWORK
    cargo build --bin quickstart -q 2>/dev/null
    echo "Build successful. Executing..."
    (
        trap - SIGINT
        # export RUST_BACKTRACE=1
        cargo run --bin quickstart -q # 2>/dev/null
    )
    echo "Program has finished or was interrupted. Continuing with the rest of the shell script ..."
    status+=($?)
    if [ $status -ne 0 ]; then
        echo "Error: $status on program ${RED}${program}${NC}"
        exit 1
    fi
    # ps -ef | grep redis-server | grep -v grep | awk '{print $2}' | xargs kill 2>/dev/null
    # rm -rf dump.rdb
}

start
