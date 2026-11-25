#!/usr/bin/env bash
# P4 soak run: sustained loopback load through the Rust core.
#
# Usage:
#   ./scripts/soak.sh [seconds]        # default 1800 (30 minutes)
#
# A loopback upstream (conformance/soak/soak_server.py) answers every native
# wire path with paced, protocol-correct fixtures. The Rust driver cycles all
# nine protocols through completion, incremental streaming, mid-stream
# cancellation, one embedding and one rerank per round, while sampling its own
# RSS and open descriptors. Exit status is non-zero on any unexpected failure
# or on suspected growth (RSS > 64 MiB, descriptors > +64).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SECONDS_TO_RUN="${1:-1800}"
LOG="${LINGXI_SOAK_LOG:-/tmp/lingxi-soak-server.log}"

(cd "$ROOT/core" && cargo build --quiet --locked --example soak)

python3 "$ROOT/conformance/soak/soak_server.py" --port 0 >"$LOG" 2>&1 &
SERVER_PID=$!

stop_server() {
  kill -TERM "$SERVER_PID" 2>/dev/null || true
  wait "$SERVER_PID" 2>/dev/null || true
}
trap stop_server EXIT

for _ in $(seq 1 100); do
  if grep -q '"url"' "$LOG" 2>/dev/null; then
    break
  fi
  sleep 0.1
done
URL="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["url"])' "$LOG")"
echo "soak upstream: $URL (${SECONDS_TO_RUN}s)"

LINGXI_SOAK_URL="$URL" LINGXI_SOAK_SECONDS="$SECONDS_TO_RUN" \
  "$ROOT/core/target/debug/examples/soak"

# The server reports accepted connections when it shuts down; a client that
# leaked sockets shows up as unbounded growth against the request count.
stop_server
trap - EXIT
grep '"final"' "$LOG" | tail -1 || true