#!/usr/bin/env bash
set -euo pipefail

if [ "${1:-}" != "" ]; then
  APP_DATA_DIR="$1"
elif [ -d "$HOME/Library/Application Support/com.fndr.app/lancedb" ]; then
  APP_DATA_DIR="$HOME/Library/Application Support/com.fndr.app"
elif [ -d "$HOME/Library/Application Support/com.fndr.FNDR/lancedb" ]; then
  APP_DATA_DIR="$HOME/Library/Application Support/com.fndr.FNDR"
elif [ -d "$HOME/Library/Application Support/FNDR/lancedb" ]; then
  APP_DATA_DIR="$HOME/Library/Application Support/FNDR"
else
  APP_DATA_DIR="$HOME/Library/Application Support/com.fndr.app"
fi

DB_DIR="$APP_DATA_DIR/lancedb"

if [ ! -d "$DB_DIR" ]; then
  echo "No LanceDB directory found."
  echo "Checked default app data location: $DB_DIR"
  echo "Pass an app data directory explicitly if FNDR is using a custom path."
  exit 0
fi

BACKUP_DIR="$APP_DATA_DIR/lancedb.backup.$(date +%Y%m%d%H%M%S)"
mv "$DB_DIR" "$BACKUP_DIR"
echo "Moved old LanceDB store to: $BACKUP_DIR"
echo "FNDR will recreate the LanceDB schema on next launch using the current embedding contract"
echo "(see src-tauri/src/inference/model_config.rs for the active model and vector dimension)."
