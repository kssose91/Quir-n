#!/usr/bin/env bash
set -euo pipefail

WORKSPACE_ROOT="${CODEX_QUIRON_ROOT:-/home/kssose/Quirón}"

exec codex -C "$WORKSPACE_ROOT" exec --skip-git-repo-check "$@"
