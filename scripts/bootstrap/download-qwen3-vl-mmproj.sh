#!/usr/bin/env bash
# Download Qwen3-VL mmproj from the official GGUF repo (filename uses Qwen3VL, no hyphen).
# Usage: ./scripts/bootstrap/download-qwen3-vl-mmproj.sh [models_dir] [Q8_0|F16]
# Default models_dir matches download-local-llm.sh (macOS app support).
set -euo pipefail

MODEL_DIR="${1:-$HOME/Library/Application Support/com.continuum.app/models}"
VARIANT="${2:-Q8_0}"

case "$VARIANT" in
  Q8_0)
    MIN_BYTES=400000000
    ;;
  F16)
    MIN_BYTES=800000000
    ;;
  *)
    echo "Second arg must be Q8_0 or F16 (got: $VARIANT)" >&2
    exit 1
    ;;
esac

mkdir -p "$MODEL_DIR"
FILE="mmproj-Qwen3VL-4B-Instruct-${VARIANT}.gguf"
URL="https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/resolve/main/${FILE}"
DEST="$MODEL_DIR/$FILE"

if [[ -f "$DEST" ]] && [[ "$(wc -c < "$DEST")" -ge "$MIN_BYTES" ]]; then
  echo "✅ mmproj already present: $DEST"
  exit 0
fi

echo "📥 Downloading $FILE from Hugging Face (~$([ "$VARIANT" = Q8_0 ] && echo 430 || echo 800) MB)..."
curl -L --fail --retry 3 --retry-delay 2 "$URL" -o "$DEST.partial"
mv "$DEST.partial" "$DEST"
echo "🎉 mmproj ready at: $DEST"
