#!/usr/bin/env bash
# Optional: download the recommended lightweight text GGUF (Llama 3.2 1B).
# Advanced users can instead download Qwen3-VL 4B from the in-app model picker.
set -euo pipefail

MODEL_DIR="${1:-$HOME/Library/Application Support/com.continuum.app/models}"
mkdir -p "$MODEL_DIR"

LLM_URL="https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/Llama-3.2-1B-Instruct-Q4_K_M.gguf"
LLM_PATH="$MODEL_DIR/Llama-3.2-1B-Instruct-Q4_K_M.gguf"

if [[ -f "$LLM_PATH" ]] && [[ "$(wc -c < "$LLM_PATH")" -ge 700000000 ]]; then
    echo "✅ Llama 3.2 1B already present."
else
    echo "📥 Downloading Llama 3.2 1B (~770MB)..."
    curl -L --fail --retry 3 --retry-delay 2 "$LLM_URL" -o "$LLM_PATH.partial"
    mv "$LLM_PATH.partial" "$LLM_PATH"
fi

echo "🎉 LLM ready at: $LLM_PATH"
