#!/usr/bin/env bash
# Download official Qwen3-VL 4B Instruct GGUF (Q4_K_M) for Continuum import vision + VLM tier.
# Usage: ./scripts/bootstrap/download-qwen3-vl-4b.sh [models_dir]
# Default models_dir matches other bootstrap scripts (macOS app support).
set -euo pipefail

MODEL_DIR="${1:-$HOME/Library/Application Support/com.continuum.app/models}"
mkdir -p "$MODEL_DIR"

FILE="Qwen3VL-4B-Instruct-Q4_K_M.gguf"
URL="https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/resolve/main/${FILE}"
DEST="$MODEL_DIR/$FILE"
# Must stay in sync with src-tauri/src/models.rs QWEN3_VL_4B_MAIN_GGUF_MIN_BYTES (reject placeholders).
MIN_BYTES=1800000000

if [[ -f "$DEST" ]] && [[ "$(wc -c < "$DEST")" -ge "$MIN_BYTES" ]]; then
  echo "✅ Qwen3-VL 4B already present: $DEST"
  exit 0
fi

if [[ -f "$DEST" ]] && [[ "$(wc -c < "$DEST")" -lt "$MIN_BYTES" ]]; then
  echo "⚠️  Removing undersized file (likely incomplete or wrong): $DEST"
  rm -f "$DEST"
fi
rm -f "$DEST.partial"

echo "📥 Downloading ${FILE} from Hugging Face (~2.3+ GiB). This can take many minutes..."
echo "   Destination: $DEST"
# Resume-friendly partial name
curl -L --fail --retry 3 --retry-delay 2 -C - "$URL" -o "$DEST.partial"
mv "$DEST.partial" "$DEST"
echo "🎉 Qwen3-VL 4B ready at: $DEST"
