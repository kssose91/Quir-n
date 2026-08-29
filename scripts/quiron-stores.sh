#!/usr/bin/env bash
#
# Ciclo de vida de los almacenes de Quirón (Qdrant y Neo4j).
#
# No arranca nada al boot: lo invoca el servicio quiron-brain en ExecStartPre
# (up) y ExecStopPost (down), de modo que los contenedores viven exactamente
# mientras vive la aplicación. Fuera de ella, no corren.
#
# Uso: quiron-stores.sh {up|down|status}
#
# Docker es rootful en esta máquina: quien ejecute esto necesita pertenecer al
# grupo `docker` (o usar sudo). El servicio systemd --user lo hereda de la sesión.

set -euo pipefail

ROOT="${QUIRON_ROOT:-/home/KSSOSE/Quirón}"

QDRANT_CONTAINER_NAME="${QUIRON_QDRANT_CONTAINER_NAME:-quiron-qdrant}"
NEO4J_CONTAINER_NAME="${QUIRON_NEO4J_CONTAINER_NAME:-quiron-neo4j}"
QDRANT_IMAGE="${QUIRON_QDRANT_IMAGE:-qdrant/qdrant}"
NEO4J_IMAGE="${QUIRON_NEO4J_IMAGE:-neo4j:5}"
QDRANT_DATA_DIR="${QUIRON_QDRANT_DATA_DIR:-${ROOT}/data/qdrant}"
NEO4J_DATA_DIR="${QUIRON_NEO4J_DATA_DIR:-${ROOT}/data/neo4j}"
NEO4J_PASSWORD="${NEO4J_PASSWORD:-neo4j}"

log() { printf '[quiron-stores] %s\n' "$*"; }
die() { printf '[quiron-stores][error] %s\n' "$*" >&2; exit 1; }

command -v docker >/dev/null 2>&1 || die "docker no está disponible"

# Nombre real del contenedor que publica un puerto dado, si existe.
discover_by_port() {
  local port="$1"
  docker ps --format '{{.Names}}\t{{.Ports}}' \
    | awk -F'\t' -v p="${port}" '$2 ~ (":" p "->" p "/tcp") { print $1; exit }'
}

is_running() { [[ -n "$(docker ps --filter "name=^/$1$" --filter status=running -q 2>/dev/null)" ]]; }
exists()     { [[ -n "$(docker ps -a --filter "name=^/$1$" -q 2>/dev/null)" ]]; }

ensure_qdrant() {
  # Reutiliza un contenedor existente aunque tenga otro nombre (descubierto por puerto).
  local discovered; discovered="$(discover_by_port 6333 || true)"
  [[ -n "${discovered}" ]] && QDRANT_CONTAINER_NAME="${discovered}"

  if is_running "${QDRANT_CONTAINER_NAME}"; then
    log "Qdrant ya está levantado (${QDRANT_CONTAINER_NAME})"
  elif exists "${QDRANT_CONTAINER_NAME}"; then
    log "Arrancando Qdrant (${QDRANT_CONTAINER_NAME})"
    docker start "${QDRANT_CONTAINER_NAME}" >/dev/null
  else
    log "Creando Qdrant"
    docker run -d --name "${QDRANT_CONTAINER_NAME}" \
      -p 6333:6333 -p 6334:6334 \
      -v "${QDRANT_DATA_DIR}:/qdrant/storage" \
      --restart no \
      "${QDRANT_IMAGE}" >/dev/null
  fi
}

ensure_neo4j() {
  local discovered; discovered="$(discover_by_port 7474 || true)"
  [[ -n "${discovered}" ]] && NEO4J_CONTAINER_NAME="${discovered}"

  if is_running "${NEO4J_CONTAINER_NAME}"; then
    log "Neo4j ya está levantado (${NEO4J_CONTAINER_NAME})"
  elif exists "${NEO4J_CONTAINER_NAME}"; then
    log "Arrancando Neo4j (${NEO4J_CONTAINER_NAME})"
    docker start "${NEO4J_CONTAINER_NAME}" >/dev/null
  else
    log "Creando Neo4j"
    docker run -d --name "${NEO4J_CONTAINER_NAME}" \
      -p 7474:7474 -p 7687:7687 \
      -e "NEO4J_AUTH=neo4j/${NEO4J_PASSWORD}" \
      -v "${NEO4J_DATA_DIR}:/data" \
      --restart no \
      "${NEO4J_IMAGE}" >/dev/null
  fi
}

stop_one() {
  local name="$1" port="$2"
  local discovered; discovered="$(discover_by_port "${port}" || true)"
  [[ -n "${discovered}" ]] && name="${discovered}"
  if is_running "${name}"; then
    log "Parando ${name}"
    docker stop "${name}" >/dev/null
  fi
}

# Espera a que un almacén acepte conexiones. Arrancar el contenedor no basta:
# Neo4j tarda en abrir el puerto bolt, y el brain fallaría al conectar.
wait_ready() {
  local label="$1" url="$2" timeout="${3:-90}"
  local start=$SECONDS
  while (( SECONDS - start < timeout )); do
    if curl -s -o /dev/null -m 3 "${url}"; then
      log "${label} listo"
      return 0
    fi
    sleep 1
  done
  log "Aviso: ${label} no respondió en ${timeout}s; se continúa"
  return 0
}

case "${1:-status}" in
  up)
    ensure_qdrant
    ensure_neo4j
    wait_ready "Qdrant" "http://127.0.0.1:6333/readyz" 30
    wait_ready "Neo4j" "http://127.0.0.1:7474" 90
    log "Almacenes arriba"
    ;;
  down)
    stop_one "${QDRANT_CONTAINER_NAME}" 6333
    stop_one "${NEO4J_CONTAINER_NAME}" 7687
    log "Almacenes parados"
    ;;
  status)
    for pair in "Qdrant:6333" "Neo4j:7474"; do
      name="${pair%%:*}"; port="${pair##*:}"
      c="$(discover_by_port "${port}" || true)"
      if [[ -n "${c}" ]]; then echo "  ${name}: corriendo (${c})"; else echo "  ${name}: parado"; fi
    done
    ;;
  *)
    die "uso: $0 {up|down|status}"
    ;;
esac
