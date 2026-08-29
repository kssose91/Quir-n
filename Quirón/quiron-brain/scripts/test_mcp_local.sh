#!/usr/bin/env bash
set -euo pipefail

ROOT="/home/kssose/Quirón/Quirón/quiron-brain"
MANIFEST="$ROOT/Cargo.toml"
BIN="$ROOT/target/release/quiron_mcp_server"
ENV_FILE="${HOME}/.config/quiron/quiron-brain.env"

if [[ -f "$ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$ENV_FILE"
  set +a
fi

export QUIRON_URL="${QUIRON_URL:-http://127.0.0.1:8766}"

if [[ ! -x "$BIN" ]]; then
  echo "Building quiron_mcp_server..."
  cargo build --release --features semantic --manifest-path "$MANIFEST" --bin quiron_mcp_server >/dev/null
fi

send_session() {
  local payload=""
  local msg
  for msg in "$@"; do
    local chunk
    printf -v chunk 'Content-Length: %s\r\n\r\n%s' "${#msg}" "$msg"
    payload+="$chunk"
  done

  printf '%s' "$payload" | QUIRON_URL="$QUIRON_URL" QUIRON_API_TOKEN="${QUIRON_API_TOKEN:-}" QUIRON_PROJECT_ROOT="${QUIRON_PROJECT_ROOT:-/home/kssose/Quirón}" "$BIN"
}

require_contains() {
  local haystack="$1"
  local needle="$2"
  local label="$3"
  if [[ "$haystack" != *"$needle"* ]]; then
    echo "FAIL: $label"
    exit 1
  fi
  echo "PASS: $label"
}

echo "== MCP smoke test =="

INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"mcp-smoke","version":"0.1"}}}'
LIST='{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
HEALTH='{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"health","arguments":{}}}'

session_output="$(send_session "$INIT" "$LIST" "$HEALTH")"
require_contains "$session_output" '"protocolVersion":"2025-03-26"' "initialize handshake"
require_contains "$session_output" '"name":"log_turn"' "tool list includes log_turn"
require_contains "$session_output" '"name":"history_worker"' "tool list includes history_worker"
require_contains "$session_output" '"service":"quiron-brain"' "health call returns quiron-brain"

if [[ -z "${QUIRON_API_TOKEN:-}" ]]; then
  echo "WARN: QUIRON_API_TOKEN is not configured; skipping log_turn and history_worker."
  exit 0
fi

LOG='{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"log_turn","arguments":{"message":"MCP local smoke turn","reply":"stored through local MCP smoke test","session_id":"mcp-local-smoke","logic_tags":["mcp_v1"],"tags":["smoke"]}}}'
log_output="$(send_session "$LOG")"
require_contains "$log_output" '"logged":true' "log_turn writes to Quiron"

event_id="$(printf '%s' "$log_output" | sed -n 's/.*"id":"\([0-9A-Z]\{26\}\)".*/\1/p' | head -n1)"
if [[ -z "$event_id" ]]; then
  echo "FAIL: could not extract event_id from log_turn output"
  exit 1
fi
echo "INFO: logged event_id=$event_id"

EVENT_GET="{\"jsonrpc\":\"2.0\",\"id\":12,\"method\":\"tools/call\",\"params\":{\"name\":\"event_get\",\"arguments\":{\"event_id\":\"$event_id\"}}}"
event_output="$(send_session "$EVENT_GET")"
require_contains "$event_output" "$event_id" "event_get returns logged event"
require_contains "$event_output" 'MCP local smoke turn' "event_get preserves logged description"

EVENT_MEMORY="{\"jsonrpc\":\"2.0\",\"id\":13,\"method\":\"tools/call\",\"params\":{\"name\":\"event_memory\",\"arguments\":{\"event_id\":\"$event_id\"}}}"
memory_output="$(send_session "$EVENT_MEMORY")"
require_contains "$memory_output" '"current"' "event_memory returns current envelope"
require_contains "$memory_output" "$event_id" "event_memory references logged event"

WORKER='{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"history_worker","arguments":{"kind":"prior_attempts","objective":"buscar intentos previos relacionados con el MCP de Quiron","query":"mcp server prior attempts","logic_tags":["mcp_v1"],"constraints":["priorizar evidencia fuente","no claims finales"]}}}'
worker_output="$(send_session "$WORKER")"
require_contains "$worker_output" '"worker_result"' "history_worker returns worker payload"
require_contains "$worker_output" '"claims":[]' "history_worker preserves no-claim contract"

echo "== MCP smoke test OK =="
