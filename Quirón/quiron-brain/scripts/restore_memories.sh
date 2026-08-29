#!/bin/bash
# Script para restaurar memorias de Quirón desde Neo4j al ledger RocksDB

API="http://127.0.0.1:8766"
COUNT=0

echo "🧠 Restaurando memorias de Quirón..."

# Extraer eventos de Neo4j y reinsertarlos
docker exec quiron-neo4j cypher-shell -u neo4j -p quiron_brain_2026 \
  "MATCH (e:Event) RETURN e.kind, e.description ORDER BY e.ts" --format plain 2>/dev/null \
| tail -n +2 \
| while IFS=',' read -r kind rest; do
  kind=$(echo "$kind" | tr -d '"' | xargs)
  desc=$(echo "$rest" | sed 's/^[[:space:]]*//' | tr -d '"' | sed 's/"/\\"/g')
  
  # Convertir kind de Neo4j a formato API
  case "$kind" in
    "FileRead") kind="FileRead" ;;
    "ClaimMade") kind="Claim" ;;  
    "PatchApplied") kind="PatchApplied" ;;
    "Observation") kind="Observation" ;;
    "Decision") kind="Decision" ;;
    "VerificationRecorded") kind="VerificationRecorded" ;;
    *) kind="Observation" ;;  # Default
  esac
  
  # Insertar via API
  result=$(curl -s -X POST "$API/event" \
    -H "Content-Type: application/json" \
    -d "{\"kind\": \"$kind\", \"description\": \"[RESTORED] $desc\"}" 2>/dev/null)
  
  if echo "$result" | grep -q '"ok":true'; then
    COUNT=$((COUNT + 1))
    echo "✓ Evento restaurado: ${desc:0:50}..."
  fi
done

echo ""
echo "✅ Restauración completada"
curl -s "$API/chain/verify" | jq .
