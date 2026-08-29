#!/bin/bash
# Migration script to add importance field to existing events
# This is a one-time migration

set -e

echo "=== Migration: Add importance field to existing events ==="

DATA_DIR="${HOME}/.quiron/brain"

if [ ! -d "$DATA_DIR" ]; then
    echo "ERROR: Data directory not found: $DATA_DIR"
    exit 1
fi

echo "1. Stopping quiron-brain service..."
systemctl --user stop quiron-brain.service || true
sleep 2

echo "2. Backing up database..."
BACKUP_DIR="${DATA_DIR}_backup_$(date +%Y%m%d_%H%M%S)"
cp -r "$DATA_DIR" "$BACKUP_DIR"
echo "   Backup created: $BACKUP_DIR"

echo "3. The migration requires rewriting all events with the new importance field."
echo "   Since bincode is a fixed-format serializer, we need to:"
echo "   a) Export all events to JSON"
echo "   b) Clear the events CF"
echo "   c) Re-import with the new struct"
echo ""
echo "   For now, we will handle this in Rust code by making importance optional."

echo "4. Restarting service..."
systemctl --user start quiron-brain.service
sleep 2

echo "Done. Please verify with: curl http://127.0.0.1:8766/health"
