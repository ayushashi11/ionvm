#!/usr/bin/env bash
set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

cleanup() {
    fuser -k 8080/tcp 2>/dev/null || true
    fuser -k 8088/tcp 2>/dev/null || true
    fuser -k 9090/tcp 2>/dev/null || true
}
trap cleanup EXIT INT TERM

wait_for_port() {
    local port=$1
    local retries=30
    while ! (echo > /dev/tcp/127.0.0.1/"$port") 2>/dev/null; do
        sleep 0.1
        retries=$((retries - 1))
        if [ "$retries" -le 0 ]; then return 1; fi
    done
}

echo "=== Running Head-to-Head Benchmark: IonVM vs Node.js ==="
echo "Requests: 5,000 | Concurrency: 50"
echo ""

# 1. IonVM Web Server (8080)
cleanup
./target/release/ionvm test_web_server.bin >/dev/null 2>&1 &
wait_for_port 8080
IONVM_WEB_RPS=$(ab -n 5000 -c 50 http://127.0.0.1:8080/ 2>/dev/null | grep "Requests per second:" | awk '{print $4}')
cleanup

# 2. Node.js Web Server (8080)
cleanup
node server_web.js >/dev/null 2>&1 &
wait_for_port 8080
NODE_WEB_RPS=$(ab -n 5000 -c 50 http://127.0.0.1:8080/ 2>/dev/null | grep "Requests per second:" | awk '{print $4}')
cleanup

# 3. IonVM Shout Server (8088)
cleanup
./target/release/ionvm test_http_shout.bin >/dev/null 2>&1 &
wait_for_port 8088
echo -n "FAST_PARSER" > /tmp/ab_bench_post.txt
IONVM_SHOUT_RPS=$(ab -n 5000 -c 50 -p /tmp/ab_bench_post.txt -T "text/plain" http://127.0.0.1:8088/ 2>/dev/null | grep "Requests per second:" | awk '{print $4}')
cleanup

# 4. Node.js Shout Server (8088)
cleanup
node server_shout.js >/dev/null 2>&1 &
wait_for_port 8088
NODE_SHOUT_RPS=$(ab -n 5000 -c 50 -p /tmp/ab_bench_post.txt -T "text/plain" http://127.0.0.1:8088/ 2>/dev/null | grep "Requests per second:" | awk '{print $4}')
cleanup
rm -f /tmp/ab_bench_post.txt

echo "RESULTS:"
echo "----------------------------------------------------------------------------------------"
printf "%-25s | %-18s | %-18s | %-10s\n" "Benchmark" "IonVM (Actor VM)" "Node.js v22 (V8)" "Speedup"
echo "----------------------------------------------------------------------------------------"
WEB_RATIO=$(awk "BEGIN {printf \"%.2f\", $IONVM_WEB_RPS / $NODE_WEB_RPS}")
printf "%-25s | %-18s | %-18s | %-10s\n" "HTTP Web Server (GET)" "${IONVM_WEB_RPS} req/s" "${NODE_WEB_RPS} req/s" "${WEB_RATIO}x faster"

SHOUT_RATIO=$(awk "BEGIN {printf \"%.2f\", $IONVM_SHOUT_RPS / $NODE_SHOUT_RPS}")
printf "%-25s | %-18s | %-18s | %-10s\n" "HTTP Shout Server (POST)" "${IONVM_SHOUT_RPS} req/s" "${NODE_SHOUT_RPS} req/s" "${SHOUT_RATIO}x faster"
echo "----------------------------------------------------------------------------------------"
