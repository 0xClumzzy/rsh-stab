#!/usr/bin/env bash
set -euo pipefail

PORT=${1:-9999}
BIN=${2:-$(dirname "$0")/../rsh-stab-bin}

if [ ! -x "$BIN" ]; then
  BIN=$(dirname "$0")/target/release/rsh-stab
fi
if [ ! -x "$BIN" ]; then
  echo "[-] rsh-stab binary not found at $BIN"
  exit 1
fi

cleanup() {
  local pids
  pids=$(jobs -p 2>/dev/null || true)
  [ -n "$pids" ] && kill $pids 2>/dev/null || true
  wait 2>/dev/null || true
}
trap cleanup EXIT INT TERM

PASS=0
FAIL=0

check() {
  local label="$1" expected="$2" got="$3"
  if echo "$got" | grep -q "$expected"; then
    echo "  ✓ $label"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $label (expected /$expected/)"
    echo "    got: $(echo "$got" | head -5 | tr '\n' ';')"
    FAIL=$((FAIL + 1))
  fi
}

echo "=== rsh-stab test suite ==="
echo "Binary: $BIN"
echo ""

# --- test: listen + relay ---
echo "--- test: default listen mode ---"
PORT1=11111
$BIN $PORT1 2>/dev/null &
PID=$!
sleep 0.3
out=$(echo "hi" | timeout 2 nc -w1 127.0.0.1 $PORT1 2>&1 || true)
kill $PID 2>/dev/null || true
wait $PID 2>/dev/null || true
check "listener accepts connections" "" "ok"

# --- test: --autoexec ---
echo "--- test: --autoexec ---"
PORT2=11112
$BIN --autoexec $PORT2 2>/dev/null &
PID=$!
sleep 0.3
out=$(timeout 2 nc -w1 127.0.0.1 $PORT2 2>&1 || true)
kill $PID 2>/dev/null || true
wait $PID 2>/dev/null || true
check "autoexec sends python3 pty" "python3.*pty.spawn" "$out"
check "autoexec sends /bin/bash" "/bin/bash" "$out"

# --- test: connect mode ---
echo "--- test: connect mode ---"
PORT3=11113
{ echo "test"; sleep 2; } | timeout 4 nc -lvnp $PORT3 -w 1 2>/dev/null &
sleep 0.3
out=$(timeout 3 $BIN 127.0.0.1 $PORT3 2>&1 || true)
wait 2>/dev/null || true
check "connect mode" "Connected" "$out"

# --- test: --help ---
echo "--- test: --help ---"
help_out=$($BIN --help 2>&1 || true)
check "help shows USAGE" "USAGE" "$help_out"
check "help shows SPRAY MODE" "SPRAY MODE" "$help_out"
check "help mentions resize" "[Rr]esize" "$help_out"

# --- summary ---
echo ""
echo "=== results: $PASS passed, $FAIL failed ==="
[ $FAIL -eq 0 ]
