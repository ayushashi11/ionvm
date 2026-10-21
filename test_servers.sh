#!/usr/bin/env bash
set -eo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

IONVM="./target/release/ionvm"

echo -e "${BOLD}${BLUE}=== IonVM Network & Web Server Test Suite ===${NC}\n"

# 1. Build IonVM release
echo -e "${CYAN}[Step 1/4] Building IonVM in release mode...${NC}"
cargo build --release --bin ionvm --quiet
echo -e "${GREEN}✓ IonVM binary ready: ${IONVM}${NC}\n"

PID_TO_KILL=""

cleanup() {
    if [ -n "$PID_TO_KILL" ]; then
        kill -9 "$PID_TO_KILL" 2>/dev/null || true
        wait "$PID_TO_KILL" 2>/dev/null || true
        PID_TO_KILL=""
    fi
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
        if [ "$retries" -le 0 ]; then
            echo -e "${RED}Failed waiting for port $port to open!${NC}"
            return 1
        fi
    done
}

# -------------------------------------------------------------
# TEST 1: Concurrent HTTP Server (Port 8080)
# -------------------------------------------------------------
echo -e "${CYAN}[Step 2/4] Testing Concurrent HTTP Server (test_web_server.bin on port 8080)...${NC}"
cleanup
$IONVM test_web_server.bin > /tmp/ionvm_web.log 2>&1 &
PID_TO_KILL=$!
wait_for_port 8080

# Verify single request
RESP=$(curl -s http://127.0.0.1:8080/)
if [[ "$RESP" == *"Hello, World!"* ]]; then
    echo -e "${GREEN}✓ Functional GET test passed: received 'Hello, World!'${NC}"
else
    echo -e "${RED}✗ GET test failed! Response: '$RESP'${NC}"
    exit 1
fi

# Run ApacheBench if installed
if command -v ab >/dev/null 2>&1; then
    echo -e "${YELLOW}  Running ApacheBench (2,000 requests, concurrency 20)...${NC}"
    AB_OUT=$(ab -n 2000 -c 20 http://127.0.0.1:8080/)
    RPS=$(echo "$AB_OUT" | grep "Requests per second:" | awk '{print $4}')
    LATENCY=$(echo "$AB_OUT" | grep "Time per request:.*(mean)" | head -n1 | awk '{print $4}')
    FAILED=$(echo "$AB_OUT" | grep "Failed requests:" | awk '{print $3}')
    echo -e "${GREEN}  ✓ Throughput: ${BOLD}${RPS} req/sec${NC} (mean latency: ${LATENCY} ms, failed: ${FAILED})"
fi
cleanup
echo ""

# -------------------------------------------------------------
# TEST 2: HTTP Shout POST Server (Port 8088)
# -------------------------------------------------------------
echo -e "${CYAN}[Step 3/4] Testing HTTP Shout Server (test_http_shout.bin on port 8088)...${NC}"
cleanup
$IONVM test_http_shout.bin > /tmp/ionvm_shout.log 2>&1 &
PID_TO_KILL=$!
wait_for_port 8088

# Verify single POST
MSG="CONCURRENT_ACTORS"
SHOUT_RESP=$(curl -s -X POST -d "$MSG" http://127.0.0.1:8088/)
EXPECTED="I WILL SHOUT ${MSG} BACK!"
if [[ "$SHOUT_RESP" == *"$EXPECTED"* ]]; then
    echo -e "${GREEN}✓ Functional POST test passed: received '$EXPECTED'${NC}"
else
    echo -e "${RED}✗ POST test failed! Response: '$SHOUT_RESP'${NC}"
    exit 1
fi

if command -v ab >/dev/null 2>&1; then
    echo -e "${YELLOW}  Running ApacheBench (2,000 POST requests, concurrency 20)...${NC}"
    echo -n "FAST_PARSER" > /tmp/ab_post.txt
    AB_OUT=$(ab -n 2000 -c 20 -p /tmp/ab_post.txt -T "text/plain" http://127.0.0.1:8088/)
    RPS=$(echo "$AB_OUT" | grep "Requests per second:" | awk '{print $4}')
    LATENCY=$(echo "$AB_OUT" | grep "Time per request:.*(mean)" | head -n1 | awk '{print $4}')
    FAILED=$(echo "$AB_OUT" | grep "Failed requests:" | awk '{print $3}')
    echo -e "${GREEN}  ✓ Throughput: ${BOLD}${RPS} req/sec${NC} (mean latency: ${LATENCY} ms, failed: ${FAILED})"
    rm -f /tmp/ab_post.txt
fi
cleanup
echo ""

# -------------------------------------------------------------
# TEST 3: Concurrent TCP Echo Server (Port 9090)
# -------------------------------------------------------------
echo -e "${CYAN}[Step 4/4] Testing TCP Echo Server (test_echo_server.bin on port 9090)...${NC}"
cleanup
$IONVM test_echo_server.bin > /tmp/ionvm_echo.log 2>&1 &
PID_TO_KILL=$!
wait_for_port 9090

# Python one-liner to connect, send, receive and verify
ECHO_RESULT=$(python3 -c '
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.connect(("127.0.0.1", 9090))
msg = b"IONVM_ACTOR_ECHO_VERIFIED\n"
s.sendall(msg)
data = s.recv(1024)
s.close()
if data == msg:
    print("SUCCESS")
else:
    print(f"MISMATCH: {data}")
')

if [ "$ECHO_RESULT" == "SUCCESS" ]; then
    echo -e "${GREEN}✓ Functional TCP Echo test passed: exact byte-level match${NC}"
else
    echo -e "${RED}✗ TCP Echo test failed: $ECHO_RESULT${NC}"
    exit 1
fi
cleanup
echo ""

# -------------------------------------------------------------
# Summary
# -------------------------------------------------------------
echo -e "${BOLD}${GREEN}======================================================${NC}"
echo -e "${BOLD}${GREEN}  ALL NETWORK & WEB SERVER TESTS PASSED SUCCESSFULLY! ${NC}"
echo -e "${BOLD}${GREEN}======================================================${NC}"
