#!/usr/bin/env bash
# Install default local models for Continuum (text embeddings + CLIP vision for imports).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
"$ROOT/scripts/bootstrap/download-embedding-model.sh" "$@"
"$ROOT/scripts/bootstrap/download-clip-vision-onnx.sh" "$@"
