#!/bin/bash
# Builds fndr-eg2-helper (FNDR's EmbeddingGemma 2 text engine) and stages it as
# src-tauri/binaries/fndr-eg2-helper-<target triple>, where Tauri's external
# binaries live. Builds the pinned llama.cpp first.
set -euo pipefail
cd "$(dirname "$0")/../.."
scripts/eg2/build-llama.sh
TARGET=$(rustc -vV | sed -n 's/^host: //p')
(cd src-tauri/helpers/fndr-eg2 && cargo build --release)
mkdir -p src-tauri/binaries
cp src-tauri/helpers/fndr-eg2/target/release/fndr-eg2-helper "src-tauri/binaries/fndr-eg2-helper-$TARGET"
echo "Staged src-tauri/binaries/fndr-eg2-helper-$TARGET"
