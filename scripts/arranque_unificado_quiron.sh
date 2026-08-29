#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SRC_ROOT="${ROOT}/Quirón"
BRAIN_ROOT="${SRC_ROOT}/quiron-brain"
VERTEX_ROOT="${SRC_ROOT}/vertex-gateway"
SEMANTIC_ROOT="${SRC_ROOT}/semantic-ia-local"

RUNTIME_DIR="${QUIRON_RUNTIME_DIR:-${ROOT}/data/runtime}"
LOG_DIR="${RUNTIME_DIR}/logs"
PID_DIR="${RUNTIME_DIR}/pids"

QDRANT_CONTAINER_NAME="${QUIRON_QDRANT_CONTAINER_NAME:-quiron-qdrant}"
NEO4J_CONTAINER_NAME="${QUIRON_NEO4J_CONTAINER_NAME:-quiron-neo4j}"
QDRANT_IMAGE="${QUIRON_QDRANT_IMAGE:-qdrant/qdrant}"
NEO4J_IMAGE="${QUIRON_NEO4J_IMAGE:-neo4j:5}"
QDRANT_DATA_DIR="${QUIRON_QDRANT_DATA_DIR:-${ROOT}/data/qdrant}"
NEO4J_DATA_DIR="${QUIRON_NEO4J_DATA_DIR:-${ROOT}/data/neo4j}"

DEFAULT_BRAIN_ENV_LOCAL="${BRAIN_ROOT}/.env.local"
DEFAULT_BRAIN_ENV_EXAMPLE="${BRAIN_ROOT}/.env.current-machine.example"

ENV_FILE=""

usage() {
  cat <<'EOF'
Uso:
  arranque_unificado_quiron.sh up
  arranque_unificado_quiron.sh down
  arranque_unificado_quiron.sh restart
  arranque_unificado_quiron.sh status
  arranque_unificado_quiron.sh logs [brain|semantic|worker|all]
  arranque_unificado_quiron.sh build

Variables utiles:
  QUIRON_ENV_FILE                    Archivo de entorno a cargar
  QUIRON_WORKER_CMD                  Comando exacto para arrancar el worker_llm
  QUIRON_START_WORKER                true|false
  QUIRON_START_SEMANTIC_IA_LOCAL     true|false
  QUIRON_FORCE_SEMANTIC_REMOTE       true|false
  QUIRON_SEMANTIC_IA_LOCAL_BIN       Ruta al binario semantic-ia-local
  QUIRON_BRAIN_BIN                   Ruta al binario quiron-brain
  QUIRON_WORKER_CUDA_VISIBLE_DEVICES GPU visible para worker_llm (default: 0)
  QUIRON_SEMANTIC_CUDA_VISIBLE_DEVICES GPU visible para semantic_ia_local (default: 1)
EOF
}

log() {
  printf '[quiron] %s\n' "$*"
}

warn() {
  printf '[quiron][warn] %s\n' "$*" >&2
}

die() {
  printf '[quiron][error] %s\n' "$*" >&2
  exit 1
}

is_true() {
  local raw="${1:-}"
  case "${raw,,}" in
    1|true|yes|on) return 0 ;;
    *) return 1 ;;
  esac
}

ensure_dirs() {
  mkdir -p "${LOG_DIR}" "${PID_DIR}" "${QDRANT_DATA_DIR}" "${NEO4J_DATA_DIR}"
}

resolve_env_file() {
  if [[ -n "${QUIRON_ENV_FILE:-}" ]]; then
    printf '%s\n' "${QUIRON_ENV_FILE}"
    return 0
  fi

  if [[ -f "${DEFAULT_BRAIN_ENV_LOCAL}" ]]; then
    printf '%s\n' "${DEFAULT_BRAIN_ENV_LOCAL}"
    return 0
  fi

  if [[ -f "${DEFAULT_BRAIN_ENV_EXAMPLE}" ]]; then
    printf '%s\n' "${DEFAULT_BRAIN_ENV_EXAMPLE}"
    return 0
  fi

  return 1
}

load_env() {
  ENV_FILE="$(resolve_env_file)" || die "No encuentro .env.local ni .env.current-machine.example"
  set -a
  # shellcheck disable=SC1090
  . "${ENV_FILE}"
  set +a

  export QUIRON_ENV_FILE="${ENV_FILE}"
  export QUIRON_VERTEX_GATEWAY_PATH="${QUIRON_VERTEX_GATEWAY_PATH:-${VERTEX_ROOT}/target/release/vertex-gateway}"
  export QUIRON_BRAIN_BIN="${QUIRON_BRAIN_BIN:-${BRAIN_ROOT}/target/release/quiron-brain}"
  export QUIRON_SEMANTIC_IA_LOCAL_BIN="${QUIRON_SEMANTIC_IA_LOCAL_BIN:-${SEMANTIC_ROOT}/target/release/semantic-ia-local}"
  export QUIRON_WORKER_CUDA_VISIBLE_DEVICES="${QUIRON_WORKER_CUDA_VISIBLE_DEVICES:-0}"
  export QUIRON_SEMANTIC_CUDA_VISIBLE_DEVICES="${QUIRON_SEMANTIC_CUDA_VISIBLE_DEVICES:-1}"
  export QUIRON_START_SEMANTIC_IA_LOCAL="${QUIRON_START_SEMANTIC_IA_LOCAL:-true}"
  export QUIRON_START_WORKER="${QUIRON_START_WORKER:-}"
  export QUIRON_FORCE_SEMANTIC_REMOTE="${QUIRON_FORCE_SEMANTIC_REMOTE:-true}"
  export QUIRON_SEMANTIC_IA_LOCAL_ENDPOINT="${QUIRON_SEMANTIC_IA_LOCAL_ENDPOINT:-http://127.0.0.1:8091}"
  export SEMANTIC_REMOTE_URL="${SEMANTIC_REMOTE_URL:-${QUIRON_SEMANTIC_IA_LOCAL_ENDPOINT}}"
  export SEMANTIC_REMOTE_TIMEOUT_SECS="${SEMANTIC_REMOTE_TIMEOUT_SECS:-${QUIRON_SEMANTIC_IA_LOCAL_TIMEOUT_SECS:-600}}"
  export NEO4J_PASSWORD="${NEO4J_PASSWORD:-password}"
}

pid_file_for() {
  printf '%s/%s.pid\n' "${PID_DIR}" "$1"
}

log_file_for() {
  printf '%s/%s.log\n' "${LOG_DIR}" "$1"
}

process_running() {
  local pid_file="$1"
  [[ -f "${pid_file}" ]] || return 1
  local pid
  pid="$(cat "${pid_file}")"
  [[ -n "${pid}" ]] || return 1
  kill -0 "${pid}" 2>/dev/null
}

cleanup_stale_pid() {
  local pid_file="$1"
  if [[ -f "${pid_file}" ]] && ! process_running "${pid_file}"; then
    rm -f "${pid_file}"
  fi
}

start_exec_process() {
  local name="$1"
  local pid_file="$2"
  local log_file="$3"
  shift 3

  cleanup_stale_pid "${pid_file}"
  if process_running "${pid_file}"; then
    log "${name} ya estaba levantado (pid $(cat "${pid_file}"))"
    return 0
  fi

  log "Arrancando ${name}"
  nohup "$@" >>"${log_file}" 2>&1 &
  local pid=$!
  echo "${pid}" >"${pid_file}"
  log "${name} arrancado (pid ${pid})"
}

start_shell_process() {
  local name="$1"
  local pid_file="$2"
  local log_file="$3"
  local command="$4"

  cleanup_stale_pid "${pid_file}"
  if process_running "${pid_file}"; then
    log "${name} ya estaba levantado (pid $(cat "${pid_file}"))"
    return 0
  fi

  log "Arrancando ${name}"
  nohup bash -lc "${command}" >>"${log_file}" 2>&1 &
  local pid=$!
  echo "${pid}" >"${pid_file}"
  log "${name} arrancado (pid ${pid})"
}

stop_process() {
  local name="$1"
  local pid_file="$2"

  cleanup_stale_pid "${pid_file}"
  if ! [[ -f "${pid_file}" ]]; then
    log "${name} no tiene pid activo"
    return 0
  fi

  local pid
  pid="$(cat "${pid_file}")"
  log "Parando ${name} (pid ${pid})"
  kill "${pid}" 2>/dev/null || true

  local waited=0
  while kill -0 "${pid}" 2>/dev/null; do
    sleep 1
    waited=$((waited + 1))
    if (( waited >= 10 )); then
      warn "${name} no cerro a tiempo; envio SIGKILL"
      kill -9 "${pid}" 2>/dev/null || true
      break
    fi
  done

  rm -f "${pid_file}"
}

docker_running() {
  docker inspect -f '{{.State.Running}}' "$1" 2>/dev/null | grep -q '^true$'
}

docker_exists() {
  docker inspect "$1" >/dev/null 2>&1
}

discover_container_by_port() {
  local port="$1"
  docker ps --format '{{.Names}}\t{{.Ports}}' \
    | awk -F'\t' -v port="${port}" '$2 ~ (":" port "->" port "/tcp") { print $1; exit }'
}

resolve_qdrant_container_name() {
  if docker_running "${QDRANT_CONTAINER_NAME}"; then
    return 0
  fi

  local discovered
  discovered="$(discover_container_by_port 6333 || true)"
  if [[ -n "${discovered}" && "${discovered}" != "${QDRANT_CONTAINER_NAME}" ]]; then
    log "Reutilizando contenedor Qdrant existente: ${discovered}"
    QDRANT_CONTAINER_NAME="${discovered}"
  fi
}

resolve_neo4j_container_name() {
  if docker_running "${NEO4J_CONTAINER_NAME}"; then
    return 0
  fi

  local discovered
  discovered="$(discover_container_by_port 7474 || true)"
  if [[ -n "${discovered}" && "${discovered}" != "${NEO4J_CONTAINER_NAME}" ]]; then
    log "Reutilizando contenedor Neo4j existente: ${discovered}"
    NEO4J_CONTAINER_NAME="${discovered}"
  fi
}

ensure_docker() {
  command -v docker >/dev/null 2>&1 || die "docker no esta disponible"
}

ensure_qdrant() {
  ensure_docker
  resolve_qdrant_container_name
  if docker_running "${QDRANT_CONTAINER_NAME}"; then
    log "Qdrant ya esta levantado"
    return 0
  fi

  if docker_exists "${QDRANT_CONTAINER_NAME}"; then
    log "Arrancando contenedor existente de Qdrant"
    docker start "${QDRANT_CONTAINER_NAME}" >/dev/null
    return 0
  fi

  log "Creando contenedor de Qdrant"
  docker run -d \
    --name "${QDRANT_CONTAINER_NAME}" \
    -p 6333:6333 \
    -p 6334:6334 \
    -v "${QDRANT_DATA_DIR}:/qdrant/storage" \
    "${QDRANT_IMAGE}" >/dev/null
}

ensure_neo4j() {
  ensure_docker
  resolve_neo4j_container_name
  if docker_running "${NEO4J_CONTAINER_NAME}"; then
    log "Neo4j ya esta levantado"
    return 0
  fi

  if docker_exists "${NEO4J_CONTAINER_NAME}"; then
    log "Arrancando contenedor existente de Neo4j"
    docker start "${NEO4J_CONTAINER_NAME}" >/dev/null
    return 0
  fi

  log "Creando contenedor de Neo4j"
  docker run -d \
    --name "${NEO4J_CONTAINER_NAME}" \
    -p 7474:7474 \
    -p 7687:7687 \
    -e "NEO4J_AUTH=neo4j/${NEO4J_PASSWORD}" \
    -v "${NEO4J_DATA_DIR}:/data" \
    "${NEO4J_IMAGE}" >/dev/null
}

stop_container_if_running() {
  local name="$1"
  if command -v docker >/dev/null 2>&1 && docker_running "${name}"; then
    log "Parando contenedor ${name}"
    docker stop "${name}" >/dev/null
  fi
}

ensure_vertex_gateway_bin() {
  local bin="${QUIRON_VERTEX_GATEWAY_PATH}"
  if [[ -x "${bin}" ]]; then
    return 0
  fi

  log "Compilando vertex-gateway"
  cargo build --release --manifest-path "${VERTEX_ROOT}/Cargo.toml"
  [[ -x "${bin}" ]] || die "No se pudo construir vertex-gateway en ${bin}"
}

ensure_brain_bin() {
  local bin="${QUIRON_BRAIN_BIN}"
  if [[ -x "${bin}" ]]; then
    return 0
  fi

  log "Compilando quiron-brain"
  cargo build --release --features full --manifest-path "${BRAIN_ROOT}/Cargo.toml"
  [[ -x "${bin}" ]] || die "No se pudo construir quiron-brain en ${bin}"
}

ensure_semantic_bin() {
  local bin="${QUIRON_SEMANTIC_IA_LOCAL_BIN}"
  if [[ -x "${bin}" ]]; then
    return 0
  fi

  log "Compilando semantic-ia-local"
  cargo build --release --manifest-path "${SEMANTIC_ROOT}/Cargo.toml"
  [[ -x "${bin}" ]] || die "No se pudo construir semantic-ia-local en ${bin}"
}

wait_http_ok() {
  local url="$1"
  local label="$2"
  local seconds="${3:-20}"

  if ! command -v curl >/dev/null 2>&1; then
    warn "curl no esta disponible; no puedo verificar ${label}"
    return 0
  fi

  local elapsed=0
  while (( elapsed < seconds )); do
    if curl -fsS "${url}" >/dev/null 2>&1; then
      log "${label} responde en ${url}"
      return 0
    fi
    sleep 1
    elapsed=$((elapsed + 1))
  done

  warn "${label} no respondio a tiempo en ${url}"
  return 1
}

endpoint_alive() {
  local url="$1"
  command -v curl >/dev/null 2>&1 || return 1
  curl -fsS "${url}" >/dev/null 2>&1
}

build_all() {
  load_env
  ensure_dirs
  ensure_vertex_gateway_bin
  ensure_brain_bin
  if is_true "${QUIRON_START_SEMANTIC_IA_LOCAL}"; then
    ensure_semantic_bin
  fi
}

start_semantic_service() {
  if ! is_true "${QUIRON_START_SEMANTIC_IA_LOCAL}"; then
    log "semantic_ia_local desactivado por entorno"
    return 0
  fi

  cleanup_stale_pid "$(pid_file_for semantic_ia_local)"
  if ! process_running "$(pid_file_for semantic_ia_local)" \
    && endpoint_alive "${QUIRON_SEMANTIC_IA_LOCAL_ENDPOINT}/health"; then
    log "semantic_ia_local ya responde fuera del script; reutilizando instancia existente"
    return 0
  fi

  ensure_semantic_bin
  start_exec_process \
    "semantic_ia_local" \
    "$(pid_file_for semantic_ia_local)" \
    "$(log_file_for semantic_ia_local)" \
    env \
      CUDA_VISIBLE_DEVICES="${QUIRON_SEMANTIC_CUDA_VISIBLE_DEVICES}" \
      "${QUIRON_SEMANTIC_IA_LOCAL_BIN}"

  wait_http_ok "${QUIRON_SEMANTIC_IA_LOCAL_ENDPOINT}/health" "semantic_ia_local" 30 || true
}

effective_semantic_backend() {
  if is_true "${QUIRON_START_SEMANTIC_IA_LOCAL}" && is_true "${QUIRON_FORCE_SEMANTIC_REMOTE}"; then
    printf 'remote\n'
  else
    printf '%s\n' "${SEMANTIC_BACKEND:-inprocess}"
  fi
}

start_worker_service() {
  local should_start_worker="false"
  if [[ -n "${QUIRON_START_WORKER}" ]]; then
    if is_true "${QUIRON_START_WORKER}"; then
      should_start_worker="true"
    fi
  elif [[ -n "${QUIRON_WORKER_CMD:-}" ]]; then
    should_start_worker="true"
  fi

  if ! is_true "${should_start_worker}"; then
    log "worker_llm omitido"
    return 0
  fi

  if [[ -z "${QUIRON_WORKER_CMD:-}" ]]; then
    warn "QUIRON_WORKER_CMD no esta definido; no puedo arrancar worker_llm"
    return 0
  fi

  local command
  command="export CUDA_VISIBLE_DEVICES='${QUIRON_WORKER_CUDA_VISIBLE_DEVICES}'; ${QUIRON_WORKER_CMD}"

  start_shell_process \
    "worker_llm" \
    "$(pid_file_for worker_llm)" \
    "$(log_file_for worker_llm)" \
    "${command}"
}

start_brain_service() {
  cleanup_stale_pid "$(pid_file_for quiron_brain)"
  if ! process_running "$(pid_file_for quiron_brain)" \
    && endpoint_alive "http://127.0.0.1:${QUIRON_PORT:-8766}/health"; then
    log "quiron-brain ya responde fuera del script; reutilizando instancia existente"
    return 0
  fi

  ensure_vertex_gateway_bin
  ensure_brain_bin

  local backend
  backend="$(effective_semantic_backend)"

  start_exec_process \
    "quiron-brain" \
    "$(pid_file_for quiron_brain)" \
    "$(log_file_for quiron_brain)" \
    env \
      QUIRON_VERTEX_GATEWAY_PATH="${QUIRON_VERTEX_GATEWAY_PATH}" \
      SEMANTIC_BACKEND="${backend}" \
      SEMANTIC_REMOTE_URL="${SEMANTIC_REMOTE_URL}" \
      SEMANTIC_REMOTE_TIMEOUT_SECS="${SEMANTIC_REMOTE_TIMEOUT_SECS}" \
      "${QUIRON_BRAIN_BIN}"

  wait_http_ok "http://127.0.0.1:${QUIRON_PORT:-8766}/health" "quiron-brain" 30 || true
}

up() {
  load_env
  ensure_dirs
  ensure_qdrant
  ensure_neo4j
  start_semantic_service
  start_worker_service
  start_brain_service
  status
}

down() {
  load_env
  stop_process "quiron-brain" "$(pid_file_for quiron_brain)"
  stop_process "worker_llm" "$(pid_file_for worker_llm)"
  stop_process "semantic_ia_local" "$(pid_file_for semantic_ia_local)"
  stop_container_if_running "${NEO4J_CONTAINER_NAME}"
  stop_container_if_running "${QDRANT_CONTAINER_NAME}"
}

status_line() {
  local label="$1"
  local value="$2"
  printf '%-18s %s\n' "${label}" "${value}"
}

status() {
  load_env
  ensure_dirs

  if command -v docker >/dev/null 2>&1; then
    resolve_qdrant_container_name
    resolve_neo4j_container_name
  fi

  printf 'Env file            %s\n' "${ENV_FILE}"
  printf 'Runtime dir         %s\n' "${RUNTIME_DIR}"
  printf 'Semantic backend    %s\n' "$(effective_semantic_backend)"
  printf 'Qdrant container    %s\n' "${QDRANT_CONTAINER_NAME}"
  printf 'Neo4j container     %s\n' "${NEO4J_CONTAINER_NAME}"

  if command -v docker >/dev/null 2>&1; then
    if docker_running "${QDRANT_CONTAINER_NAME}"; then
      status_line "qdrant" "running"
    else
      status_line "qdrant" "stopped-or-missing"
    fi
    if docker_running "${NEO4J_CONTAINER_NAME}"; then
      status_line "neo4j" "running"
    else
      status_line "neo4j" "stopped-or-missing"
    fi
  else
    status_line "docker" "not-installed"
  fi

  local service
  for service in semantic_ia_local worker_llm quiron_brain; do
    local pid_file
    pid_file="$(pid_file_for "${service}")"
    cleanup_stale_pid "${pid_file}"
    if process_running "${pid_file}"; then
      status_line "${service}" "running pid $(cat "${pid_file}")"
    else
      status_line "${service}" "stopped"
    fi
  done

  if command -v curl >/dev/null 2>&1; then
    if curl -fsS "${QUIRON_SEMANTIC_IA_LOCAL_ENDPOINT}/health" >/dev/null 2>&1; then
      status_line "semantic endpoint" "ok"
    else
      status_line "semantic endpoint" "down"
    fi
    if curl -fsS "http://127.0.0.1:${QUIRON_PORT:-8766}/health" >/dev/null 2>&1; then
      status_line "brain endpoint" "ok"
    else
      status_line "brain endpoint" "down"
    fi
  fi
}

show_logs() {
  local target="${1:-all}"
  case "${target}" in
    brain)
      tail -n 80 "$(log_file_for quiron_brain)"
      ;;
    semantic)
      tail -n 80 "$(log_file_for semantic_ia_local)"
      ;;
    worker)
      tail -n 80 "$(log_file_for worker_llm)"
      ;;
    all)
      printf '== semantic_ia_local ==\n'
      tail -n 40 "$(log_file_for semantic_ia_local)" 2>/dev/null || true
      printf '\n== worker_llm ==\n'
      tail -n 40 "$(log_file_for worker_llm)" 2>/dev/null || true
      printf '\n== quiron-brain ==\n'
      tail -n 40 "$(log_file_for quiron_brain)" 2>/dev/null || true
      ;;
    *)
      die "Objetivo de logs desconocido: ${target}"
      ;;
  esac
}

main() {
  local command="${1:-status}"
  case "${command}" in
    up)
      up
      ;;
    down)
      down
      ;;
    restart)
      down
      up
      ;;
    status)
      status
      ;;
    logs)
      show_logs "${2:-all}"
      ;;
    build)
      build_all
      ;;
    help|-h|--help)
      usage
      ;;
    *)
      usage
      exit 1
      ;;
  esac
}

main "$@"
