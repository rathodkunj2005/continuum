#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MANIFEST_PATH="$REPO_ROOT/src-tauri/Cargo.toml"
TARGET_DIR="$REPO_ROOT/src-tauri/target"
RUNTIME_DIR="${HOME}/Library/Application Support/com.continuum.app"

DRY_RUN=0
ASSUME_YES=0
CLEAN_RUNTIME=0
CLEAN_MODELS=0

usage() {
    cat <<USAGE
Usage: scripts/clean-dev-build-cache.sh [--dry-run] [--yes] [--runtime] [--models] [--all]

Safely removes Rust/Tauri developer build artifacts with cargo clean.
By default this does not delete Continuum runtime data, memory cards, LanceDB,
summaries, models, screenshots, or app settings.

Options:
  --dry-run   Show what would be cleaned without deleting anything.
  --yes       Run without prompting.
  --runtime   Also delete local runtime memory data, LanceDB, backups, frames, meetings, and voice cache.
  --models    Also delete downloaded local model blobs.
  --all       Delete build cache, runtime data, and downloaded local models.
  --help      Show this help.
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --yes|-y)
            ASSUME_YES=1
            shift
            ;;
        --runtime)
            CLEAN_RUNTIME=1
            shift
            ;;
        --models)
            CLEAN_MODELS=1
            shift
            ;;
        --all)
            CLEAN_RUNTIME=1
            CLEAN_MODELS=1
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

size_of() {
    local path="$1"
    if [[ -e "$path" ]]; then
        du -sh "$path" 2>/dev/null | awk '{print $1}'
    else
        echo "0B"
    fi
}

size_of_glob() {
    local total
    total="$(du -sch "$@" 2>/dev/null | awk '/total$/ {print $1}' || true)"
    if [[ -n "$total" ]]; then
        echo "$total"
    else
        echo "0B"
    fi
}

echo "Continuum developer build cache cleanup"
echo
echo "Build cache target:"
echo "  src-tauri/target: $(size_of "$TARGET_DIR")"
echo "  debug:            $(size_of "$TARGET_DIR/debug")"
echo "  release:          $(size_of "$TARGET_DIR/release")"
echo "  frontend dist:    $(size_of "$REPO_ROOT/dist")"
echo
echo "Runtime data:"
echo "  app data:         $(size_of "$RUNTIME_DIR")"
echo "  memory DB:        $(size_of "$RUNTIME_DIR/lancedb")"
echo "  frames:           $(size_of "$RUNTIME_DIR/frames")"
echo "  DB backups:       $(size_of_glob "$RUNTIME_DIR"/lancedb.backup.*)"
echo "  models:           $(size_of "$RUNTIME_DIR/models")"
echo "  speech models:    $(size_of "$RUNTIME_DIR/speech_models")"
echo

if [[ ! -f "$MANIFEST_PATH" ]]; then
    echo "Could not find Cargo manifest at $MANIFEST_PATH" >&2
    exit 1
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "Dry run only. To clean build cache: npm run clean:dev-cache"
    echo "To remove runtime data and downloaded models too: scripts/clean-dev-build-cache.sh --yes --all"
    exit 0
fi

if [[ "$ASSUME_YES" -ne 1 ]]; then
    printf "Remove selected Continuum generated artifacts now? This will make the next build/model launch slower. [y/N] "
    read -r reply
    case "$reply" in
        y|Y|yes|YES)
            ;;
        *)
            echo "Cancelled."
            exit 0
            ;;
    esac
fi

if [[ -e "$TARGET_DIR" ]]; then
    cargo clean --manifest-path "$MANIFEST_PATH"
else
    echo "No Rust/Tauri target directory to clean."
fi

rm -rf "$REPO_ROOT/dist"

if [[ "${CLEAN_RUNTIME:-0}" -eq 1 ]]; then
    rm -rf \
        "$RUNTIME_DIR/lancedb" \
        "$RUNTIME_DIR"/lancedb.backup.* \
        "$RUNTIME_DIR/frames" \
        "$RUNTIME_DIR/meetings" \
        "$RUNTIME_DIR/voice" \
        "$RUNTIME_DIR/memory_graph.json.migrated" \
        "$RUNTIME_DIR/tasks.json.migrated" \
        "$RUNTIME_DIR/memory_repair_progress.json" \
        "$RUNTIME_DIR/memory_repair_checkpoint.json" \
        "$RUNTIME_DIR/storage_reclaim_progress.json"
fi

if [[ "${CLEAN_MODELS:-0}" -eq 1 ]]; then
    rm -rf \
        "$RUNTIME_DIR/models" \
        "$RUNTIME_DIR/speech_models" \
        "$REPO_ROOT/src-tauri/models" \
        "${HOME}/Library/Application Support/com.continuum.Continuum/models"
fi

rm -rf "${HOME}/Library/Caches/com.continuum.app" "${HOME}/Library/Caches/continuum"

echo
echo "Cleanup complete."
echo "  repo:             $(size_of "$REPO_ROOT")"
echo "  src-tauri/target: $(size_of "$TARGET_DIR")"
echo "  memory DB:        $(size_of "$RUNTIME_DIR/lancedb")"
echo "  app data:         $(size_of "$RUNTIME_DIR")"
