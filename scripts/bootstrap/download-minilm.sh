#!/usr/bin/env bash
set -euo pipefail

TARGET_DIR_DEFAULT="$HOME/Library/Application Support/com.fndr.FNDR/models"
TARGET_DIR="${1:-$TARGET_DIR_DEFAULT}"
MODEL_PATH="$TARGET_DIR/all-MiniLM-L6-v2.onnx"
TOKENIZER_PATH="$TARGET_DIR/tokenizer.json"

mkdir -p "$TARGET_DIR"

echo "🔄 Downloading All-MiniLM-L6-v2 to: $TARGET_DIR"

export FNDR_MODEL_TARGET_DIR="$TARGET_DIR"

python3 << 'PYEOF'
from huggingface_hub import hf_hub_download
import os
import shutil
import sys

target_dir = os.path.expanduser(os.environ.get("FNDR_MODEL_TARGET_DIR", ""))
if not target_dir:
    print("❌ FNDR_MODEL_TARGET_DIR is not set")
    sys.exit(1)
os.makedirs(target_dir, exist_ok=True)

print("📥 Downloading all-MiniLM-L6-v2.onnx...")
try:
    # Download to temp location to get the directory structure
    model_path = hf_hub_download(
        repo_id="Xenova/all-MiniLM-L6-v2",
        filename="onnx/model.onnx",
        repo_type="model",
        local_dir=target_dir
    )
    # Move from onnx/model.onnx to all-MiniLM-L6-v2.onnx
    final_model_path = os.path.join(target_dir, "all-MiniLM-L6-v2.onnx")
    if model_path != final_model_path:
        shutil.move(model_path, final_model_path)
    size_mb = os.path.getsize(final_model_path) / 1e6
    print(f"   ✅ Model: {size_mb:.1f} MB")
except Exception as e:
    print(f"   ❌ Failed: {e}")
    sys.exit(1)

print("📥 Downloading tokenizer.json...")
try:
    tokenizer_path = hf_hub_download(
        repo_id="Xenova/all-MiniLM-L6-v2",
        filename="tokenizer.json",
        repo_type="model",
        local_dir=target_dir
    )
    size_kb = os.path.getsize(tokenizer_path) / 1e3
    print(f"   ✅ Tokenizer: {size_kb:.1f} KB")
except Exception as e:
    print(f"   ❌ Failed: {e}")
    sys.exit(1)

# Clean up empty subdirectories
onnx_dir = os.path.join(target_dir, "onnx")
if os.path.isdir(onnx_dir) and not os.listdir(onnx_dir):
    os.rmdir(onnx_dir)

print("\n🎉 All-MiniLM-L6-v2 ready!")
print("   📊 Model: ~90 MB, uses ~0.5 GB RAM")
PYEOF
