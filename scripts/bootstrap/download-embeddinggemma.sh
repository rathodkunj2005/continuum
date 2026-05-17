#!/usr/bin/env bash
set -euo pipefail

TARGET_DIR_DEFAULT="$HOME/Library/Application Support/com.fndr.FNDR/models"
TARGET_DIR="${1:-$TARGET_DIR_DEFAULT}"
MODEL_PATH="$TARGET_DIR/embeddinggemma-300m.onnx"
TOKENIZER_PATH="$TARGET_DIR/tokenizer.json"

mkdir -p "$TARGET_DIR"

download_if_needed() {
  local repo_id="$1"
  local filename="$2"
  local output="$3"
  local min_bytes="$4"

  if [ -f "$output" ] && [ "$(wc -c < "$output")" -ge "$min_bytes" ]; then
    echo "✅ $(basename "$output") already present."
    return
  fi

  echo "📥 Downloading $(basename "$output") from Hugging Face..."
  python3 << 'EOF'
from huggingface_hub import hf_hub_download
import sys

repo_id = sys.argv[1]
filename = sys.argv[2]
local_dir = sys.argv[3]

try:
    path = hf_hub_download(
        repo_id=repo_id,
        filename=filename,
        repo_type="model",
        local_dir=local_dir,
        force_download=False
    )
    print(f"   Downloaded to: {path}")
except Exception as e:
    print(f"   Error: {e}")
    sys.exit(1)
EOF
  "$@" "$repo_id" "$filename" "$TARGET_DIR" || {
    echo "❌ Failed to download $(basename "$output")"
    exit 1
  }
}

echo "🔄 Downloading EmbeddingGemma-300M model to: $TARGET_DIR"

# Download model using huggingface_hub Python API
python3 << 'PYEOF'
from huggingface_hub import hf_hub_download
import os

repo_id = "Xenova/embedding-gemma-300m"
target_dir = os.path.expanduser("$HOME/Library/Application Support/com.fndr.FNDR/models")
os.makedirs(target_dir, exist_ok=True)

print("📥 Downloading embeddinggemma-300m.onnx...")
try:
    model_path = hf_hub_download(
        repo_id=repo_id,
        filename="onnx/model.onnx",
        repo_type="model",
        local_dir=target_dir
    )
    print(f"   ✅ Model downloaded")
except Exception as e:
    print(f"   ❌ Failed: {e}")
    exit(1)

print("📥 Downloading tokenizer.json...")
try:
    tokenizer_path = hf_hub_download(
        repo_id=repo_id,
        filename="tokenizer.json",
        repo_type="model",
        local_dir=target_dir
    )
    print(f"   ✅ Tokenizer downloaded")
except Exception as e:
    print(f"   ❌ Failed: {e}")
    exit(1)

print("\n🎉 EmbeddingGemma-300M ready at: $TARGET_DIR")
PYEOF
