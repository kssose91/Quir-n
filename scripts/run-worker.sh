#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKER_DIR="${QUIRON_LOCAL_WORKER_DIR:-${ROOT}/data/worker}"
SERVER="$(python3 - "$WORKER_DIR/manifest.json" "$ROOT" <<'PY'
import json,sys,pathlib
manifest=json.load(open(sys.argv[1]))
base=pathlib.Path(sys.argv[1]).parent if manifest.get('manifest_version')==2 else pathlib.Path(sys.argv[2])
print(base/manifest['server'])
PY
)"
if [[ -z "${QUIRON_API_TOKEN:-}" ]]; then
  echo 'Falta QUIRON_API_TOKEN: arranca el worker con el mismo EnvironmentFile que quiron-brain.' >&2
  exit 1
fi
export LLAMA_API_KEY="$QUIRON_API_TOKEN"
DEVICE="${QUIRON_WORKER_DEVICE:-}"
if [[ -z "$DEVICE" ]]; then
  DEVICE="$("$SERVER" --list-devices 2>/dev/null | awk '/NVIDIA/ {gsub(":", "", $1); print $1; exit}')"
  DEVICE="${DEVICE:-none}"
fi
# La caché de prompts de llama-server vive en la RAM del host y por defecto crece
# hasta 8 GiB (b10816). En el barrido del 05-09-2026 llegó a 7,8 GB y dejó el
# portátil sin memoria. Con un solo slot basta un tope pequeño.
exec "$SERVER" --device "$DEVICE" --model "$WORKER_DIR/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf" \
  --alias quiron-worker --host 127.0.0.1 --port "${QUIRON_LOCAL_WORKER_PORT:-8092}" \
  --ctx-size 4096 --parallel 1 --n-gpu-layers "${QUIRON_WORKER_GPU_LAYERS:-99}" \
  --cache-ram "${QUIRON_WORKER_CACHE_RAM_MIB:-256}" \
  --threads "${QUIRON_WORKER_THREADS:-4}" --no-webui
