#!/usr/bin/env bash
# Almacenes a demanda. Nunca adopta ni detiene un contenedor por su puerto.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${QUIRON_ROOT:-$(cd "${SCRIPT_DIR}/.." && pwd)}"
QDRANT_CONTAINER_NAME="${QUIRON_QDRANT_CONTAINER_NAME:-quiron-qdrant}"
NEO4J_CONTAINER_NAME="${QUIRON_NEO4J_CONTAINER_NAME:-quiron-neo4j}"
# Digests presentes en el equipo auditado el 05-09-2026.
QDRANT_IMAGE="${QUIRON_QDRANT_IMAGE:-qdrant/qdrant@sha256:0425e3e03e7fd9b3dc95c4214546afe19de2eb2e28ca621441a56663ac6e1f46}"
NEO4J_IMAGE="${QUIRON_NEO4J_IMAGE:-neo4j@sha256:037cf5756f0135cbfd66b739b6df7c7c4bb100f9ce11602f6f9538e17e02c74d}"
QDRANT_DATA_DIR="${QUIRON_QDRANT_DATA_DIR:-${ROOT}/data/qdrant}"
NEO4J_DATA_DIR="${QUIRON_NEO4J_DATA_DIR:-${ROOT}/data/neo4j}"
READY_TIMEOUT="${QUIRON_STORES_TIMEOUT:-90}"
# Topes de RAM por contenedor (cgroup, sin swap). Neo4j: heap 512m + pagecache
# 256m + JVM. Qdrant mapea sus segmentos en memoria; el exceso es reclamable.
QDRANT_MEMORY="${QUIRON_QDRANT_MEMORY:-2g}"
NEO4J_MEMORY="${QUIRON_NEO4J_MEMORY:-1536m}"
NEO4J_PASSWORD="${NEO4J_PASSWORD:-}"

log() { printf '[quiron-stores] %s\n' "$*"; }
die() { printf '[quiron-stores][error] %s\n' "$*" >&2; exit 1; }
exists() { docker container inspect "$1" >/dev/null 2>&1; }
is_running() { [[ "$(docker inspect -f '{{.State.Running}}' "$1")" == true ]]; }
# Un contenedor creado por otra vía (p. ej. un compose antiguo con
# `unless-stopped`) heredaría su política y correría sin tope: se corrige antes
# de arrancarlo. `docker update` es idempotente y no toca los datos.
enforce_limits() { docker update --restart no --memory "$2" --memory-swap "$2" "$1" >/dev/null; }

ensure_qdrant() {
  if exists "$QDRANT_CONTAINER_NAME"; then
    enforce_limits "$QDRANT_CONTAINER_NAME" "$QDRANT_MEMORY"
    is_running "$QDRANT_CONTAINER_NAME" || docker start "$QDRANT_CONTAINER_NAME" >/dev/null
  else
    mkdir -p "$QDRANT_DATA_DIR"
    docker run -d --name "$QDRANT_CONTAINER_NAME" \
      --label org.quiron.store=qdrant --restart no \
      --memory "$QDRANT_MEMORY" --memory-swap "$QDRANT_MEMORY" \
      -p 127.0.0.1:6333:6333 -p 127.0.0.1:6334:6334 \
      -v "${QDRANT_DATA_DIR}:/qdrant/storage" "$QDRANT_IMAGE" >/dev/null
  fi
}

ensure_neo4j() {
  if exists "$NEO4J_CONTAINER_NAME"; then
    enforce_limits "$NEO4J_CONTAINER_NAME" "$NEO4J_MEMORY"
    is_running "$NEO4J_CONTAINER_NAME" || docker start "$NEO4J_CONTAINER_NAME" >/dev/null
  else
    mkdir -p "$NEO4J_DATA_DIR"
    # Docker lee el valor del entorno; la contraseña no aparece en argv.
    export NEO4J_AUTH="neo4j/${NEO4J_PASSWORD}"
    docker run -d --name "$NEO4J_CONTAINER_NAME" \
      --label org.quiron.store=neo4j --restart no \
      --memory "$NEO4J_MEMORY" --memory-swap "$NEO4J_MEMORY" \
      -p 127.0.0.1:7474:7474 -p 127.0.0.1:7687:7687 \
      -e NEO4J_AUTH \
      -e NEO4J_server_memory_heap_initial__size=256m \
      -e NEO4J_server_memory_heap_max__size=512m \
      -e NEO4J_server_memory_pagecache_size=256m \
      -v "${NEO4J_DATA_DIR}:/data" "$NEO4J_IMAGE" >/dev/null
  fi
}

qdrant_ready() {
  is_running "$QDRANT_CONTAINER_NAME" &&
    curl --fail --silent --show-error --max-time 3 http://127.0.0.1:6333/readyz >/dev/null 2>&1
}

neo4j_ready() {
  # HTTP 200 no garantiza que Bolt acepte consultas autenticadas.
  is_running "$NEO4J_CONTAINER_NAME" &&
    timeout 8 docker exec -e NEO4J_PASSWORD -e NEO4J_USERNAME=neo4j \
      "$NEO4J_CONTAINER_NAME" cypher-shell -a bolt://127.0.0.1:7687 \
      'RETURN 1;' >/dev/null 2>&1
}

wait_ready() {
  local label="$1" check="$2" start=$SECONDS
  while (( SECONDS - start < READY_TIMEOUT )); do
    if "$check"; then log "$label listo"; return 0; fi
    sleep 1
  done
  die "$label no está listo tras ${READY_TIMEOUT}s; no se inicia el cerebro"
}

stop_one() {
  if exists "$1" && is_running "$1"; then
    log "Parando $1"
    docker stop --time 20 "$1" >/dev/null
  fi
}

action="${1:-status}"
case "$action" in up|down|status) ;; *) die "uso: $0 {up|down|status}" ;; esac
command -v docker >/dev/null || die "docker no está disponible"
docker info >/dev/null 2>&1 || die "Docker no responde o el usuario no tiene acceso a su socket"

case "$action" in
  up)
    command -v curl >/dev/null || die "falta curl"
    command -v timeout >/dev/null || die "falta timeout (coreutils)"
    [[ "$READY_TIMEOUT" =~ ^[1-9][0-9]*$ ]] || die "QUIRON_STORES_TIMEOUT debe ser un entero positivo"
    [[ ${#NEO4J_PASSWORD} -ge 8 ]] || die "NEO4J_PASSWORD debe contener al menos 8 caracteres"
    export NEO4J_PASSWORD
    ensure_qdrant
    ensure_neo4j
    wait_ready Qdrant qdrant_ready
    wait_ready Neo4j neo4j_ready
    log "Almacenes listos"
    ;;
  down)
    stop_one "$QDRANT_CONTAINER_NAME"
    stop_one "$NEO4J_CONTAINER_NAME"
    ;;
  status)
    for name in "$QDRANT_CONTAINER_NAME" "$NEO4J_CONTAINER_NAME"; do
      if exists "$name" && is_running "$name"; then
        log "$name: corriendo (salud no comprobada)"
      else
        log "$name: parado o ausente"
      fi
    done
    ;;
esac
