#!/usr/bin/env bash
# CLIP ViT-B/32 vision tower (quantized ONNX) for Meta glasses / photo import embeddings.
# Output: clip-vit-base-patch32-vision_q4.onnx in the target models directory (~64 MB).
set -euo pipefail

TARGET_DIR_DEFAULT="$HOME/Library/Application Support/com.continuum.app/models"
TARGET_DIR="${1:-$TARGET_DIR_DEFAULT}"
OUT_NAME="clip-vit-base-patch32-vision_q4.onnx"
CLIP_URL="https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/main/onnx/vision_model_q4.onnx"
MIN_BYTES=50000000

mkdir -p "$TARGET_DIR"
DEST="$TARGET_DIR/$OUT_NAME"

if [[ -f "$DEST" ]] && [[ "$(wc -c < "$DEST")" -ge "$MIN_BYTES" ]]; then
  echo "✅ $OUT_NAME already present."
  exit 0
fi

echo "📥 Downloading CLIP vision ONNX to $DEST ..."
curl -L --fail --retry 3 --retry-delay 2 "$CLIP_URL" -o "$DEST.partial"
mv "$DEST.partial" "$DEST"
echo "🎉 CLIP vision model ready: $DEST"
