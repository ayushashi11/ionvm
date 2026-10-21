#!/usr/bin/env bash
set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

cleanup() {
    fuser -k 8080/tcp 2>/dev/null || true
    fuser -k 8088/tcp 2>/dev/null || true
    fuser -k 9090/tcp 2>/dev/null || true
    sleep 0.1
}
trap cleanup EXIT INT TERM

wait_for_port() {
    local port=$1
    local retries=40
    while ! (echo > /dev/tcp/127.0.0.1/"$port") 2>/dev/null; do
        sleep 0.1
        retries=$((retries - 1))
        if [ "$retries" -le 0 ]; then return 1; fi
    done
}

echo "=========================================================================="
echo "   THREE-WAY WEB SERVER BENCHMARK: IonVM vs Node.js v22 vs Java 25 Loom   "
echo "   Workload: 5,000 requests | Concurrency: 50 | Client: ApacheBench       "
echo "=========================================================================="
echo ""

# -------------------------------------------------------------
# 1. HTTP Web Server (GET on Port 8080)
# -------------------------------------------------------------
echo "[1/3] Benchmarking HTTP Web Server (GET)..."

# IonVM
cleanup
./target/release/ionvm test_web_server.bin >/dev/null 2>&1 &
wait_for_port 8080
ab -n 500 -c 10 http://127.0.0.1:8080/ >/dev/null 2>&1 # warmup
IONVM_WEB_RPS=$(ab -n 5000 -c 50 http://127.0.0.1:8080/ 2>/dev/null | grep "Requests per second:" | awk '{print $4}')
cleanup

# Node.js
cleanup
node server_web.js >/dev/null 2>&1 &
wait_for_port 8080
ab -n 500 -c 10 http://127.0.0.1:8080/ >/dev/null 2>&1 # warmup
NODE_WEB_RPS=$(ab -n 5000 -c 50 http://127.0.0.1:8080/ 2>/dev/null | grep "Requests per second:" | awk '{print $4}')
cleanup

# Java 25 (Virtual Threads)
cleanup
java ServerWeb.java >/dev/null 2>&1 &
wait_for_port 8080
ab -n 500 -c 10 http://127.0.0.1:8080/ >/dev/null 2>&1 # warmup
JAVA_WEB_RPS=$(ab -n 5000 -c 50 http://127.0.0.1:8080/ 2>/dev/null | grep "Requests per second:" | awk '{print $4}')
cleanup

# -------------------------------------------------------------
# 2. HTTP Shout Server (POST on Port 8088)
# -------------------------------------------------------------
echo "[2/3] Benchmarking HTTP Shout Server (POST body parsing & concat)..."
echo -n "FAST_PARSER_PAYLOAD" > /tmp/ab_bench_post.txt

# IonVM
cleanup
./target/release/ionvm test_http_shout.bin >/dev/null 2>&1 &
wait_for_port 8088
ab -n 500 -c 10 -p /tmp/ab_bench_post.txt -T "text/plain" http://127.0.0.1:8088/ >/dev/null 2>&1 # warmup
IONVM_SHOUT_RPS=$(ab -n 5000 -c 50 -p /tmp/ab_bench_post.txt -T "text/plain" http://127.0.0.1:8088/ 2>/dev/null | grep "Requests per second:" | awk '{print $4}')
cleanup

# Node.js
cleanup
node server_shout.js >/dev/null 2>&1 &
wait_for_port 8088
ab -n 500 -c 10 -p /tmp/ab_bench_post.txt -T "text/plain" http://127.0.0.1:8088/ >/dev/null 2>&1 # warmup
NODE_SHOUT_RPS=$(ab -n 5000 -c 50 -p /tmp/ab_bench_post.txt -T "text/plain" http://127.0.0.1:8088/ 2>/dev/null | grep "Requests per second:" | awk '{print $4}')
cleanup

# Java 25 (Virtual Threads)
cleanup
java ServerShout.java >/dev/null 2>&1 &
wait_for_port 8088
ab -n 500 -c 10 -p /tmp/ab_bench_post.txt -T "text/plain" http://127.0.0.1:8088/ >/dev/null 2>&1 # warmup
JAVA_SHOUT_RPS=$(ab -n 5000 -c 50 -p /tmp/ab_bench_post.txt -T "text/plain" http://127.0.0.1:8088/ 2>/dev/null | grep "Requests per second:" | awk '{print $4}')
cleanup
rm -f /tmp/ab_bench_post.txt

# -------------------------------------------------------------
# 3. TCP Echo Server Verification (Port 9090)
# -------------------------------------------------------------
echo "[3/3] Verifying TCP Echo Server..."

verify_echo() {
    python3 -c '
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.connect(("127.0.0.1", 9090))
msg = b"ECHO_VERIFY_PAYLOAD_12345\n"
s.sendall(msg)
data = s.recv(1024)
s.close()
assert data == msg
'
}

cleanup
./target/release/ionvm test_echo_server.bin >/dev/null 2>&1 &
wait_for_port 9090
verify_echo && IONVM_ECHO="Pass" || IONVM_ECHO="Fail"
cleanup

cleanup
node server_echo.js >/dev/null 2>&1 &
wait_for_port 9090
verify_echo && NODE_ECHO="Pass" || NODE_ECHO="Fail"
cleanup

cleanup
java ServerEcho.java >/dev/null 2>&1 &
wait_for_port 9090
verify_echo && JAVA_ECHO="Pass" || JAVA_ECHO="Fail"
cleanup

echo ""
echo "=========================================================================================================="
printf "%-25s | %-20s | %-20s | %-20s\n" "Benchmark" "IonVM (Actor VM)" "Node.js v22 (V8)" "Java 25 (Loom Virtual)"
echo "----------------------------------------------------------------------------------------------------------"
printf "%-25s | %-20s | %-20s | %-20s\n" "HTTP Web Server (GET)" "${IONVM_WEB_RPS} req/s" "${NODE_WEB_RPS} req/s" "${JAVA_WEB_RPS} req/s"
printf "%-25s | %-20s | %-20s | %-20s\n" "HTTP Shout (POST)"     "${IONVM_SHOUT_RPS} req/s" "${NODE_SHOUT_RPS} req/s" "${JAVA_SHOUT_RPS} req/s"
printf "%-25s | %-20s | %-20s | %-20s\n" "TCP Echo Verification"  "${IONVM_ECHO}"            "${NODE_ECHO}"            "${JAVA_ECHO}"
echo "=========================================================================================================="
