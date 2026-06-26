#!/usr/bin/env bash
# Stage the small, essential embedding models that ship *inside* the .app so a
# fresh install has text + image search working on first launch with no network.
#
# These are copied into <Resources>/models/ by Tauri (see tauri.conf.json
# `bundle.resources`) and seeded into the user's app-data models dir on first
# run (see seed_bundled_models in src-tauri/src/main.rs). The large Qwen3-VL
# GGUF + mmproj are NOT staged here — they are downloaded in-app during
# onboarding because of their size.
#
# Usage: scripts/release/stage-bundled-models.sh [target_dir]
#   target_dir defaults to src-tauri/bundled-models (the Tauri resource source).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TARGET_DIR="${1:-$ROOT/src-tauri/bundled-models}"
mkdir -p "$TARGET_DIR"

# Filenames here MUST match what the runtime resolvers expect:
#   - EMBEDDING_MODEL_FILENAME / EMBEDDING_TOKENIZER_FILENAME (model_config.rs)
#   - CLIP_VISION_ONNX_FILENAME (embedding/clip_vision.rs)
MINILM_URL="https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx"
MINILM_OUT="all-MiniLM-L6-v2.onnx"
MINILM_MIN_BYTES=50000000

TOKENIZER_URL="https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main/tokenizer.json"
TOKENIZER_OUT="tokenizer.json"
TOKENIZER_MIN_BYTES=100000

CLIP_URL="https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/main/onnx/vision_model_q4.onnx"
CLIP_OUT="clip-vit-base-patch32-vision_q4.onnx"
CLIP_MIN_BYTES=50000000

download_if_needed() {
  local url="$1" out="$2" min_bytes="$3"
  local dest="$TARGET_DIR/$out"
  if [[ -f "$dest" ]] && [[ "$(wc -c < "$dest")" -ge "$min_bytes" ]]; then
    echo "✅ $out already staged ($(wc -c < "$dest") bytes)."
    return
  fi
  echo "📥 Downloading $out ..."
  curl -L --fail --retry 3 --retry-delay 2 "$url" -o "$dest.partial"
  mv "$dest.partial" "$dest"
  local size
  size="$(wc -c < "$dest")"
  if [[ "$size" -lt "$min_bytes" ]]; then
    echo "❌ $out is only $size bytes (expected >= $min_bytes). Aborting." >&2
    exit 1
  fi
  echo "   ✅ $out staged ($size bytes)."
}

echo "Staging bundled models into: $TARGET_DIR"
download_if_needed "$MINILM_URL" "$MINILM_OUT" "$MINILM_MIN_BYTES"
download_if_needed "$TOKENIZER_URL" "$TOKENIZER_OUT" "$TOKENIZER_MIN_BYTES"
download_if_needed "$CLIP_URL" "$CLIP_OUT" "$CLIP_MIN_BYTES"
echo "🎉 Bundled models ready in $TARGET_DIR"
