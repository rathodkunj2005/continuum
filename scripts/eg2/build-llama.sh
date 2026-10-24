#!/bin/bash
# Builds llama.cpp for FNDR's EmbeddingGemma 2 helper: the pinned tag plus
# llama.cpp-no-logits.patch, as one static library with its headers, in
# src-tauri/helpers/fndr-eg2/vendor/llama (out of git). Ported from DigUp's
# scripts/build-llama.sh (MIT). Qwen keeps the llama.cpp in llama-cpp-2; the
# two never share a binary.
#
# A new TAG ships only if the reference-vector check passes
# (`fndr-eg2-helper --check`), or VECTOR_VERSION in the helper is bumped and every
# memory is embedded again.
set -euo pipefail
cd "$(dirname "$0")/../.."
TAG=b11461     # the first tags with EmbeddingGemma 2 are b11452+
COMMIT=4d756bc72bf00a4aacf410ae15a2d315f3db400d   # a tag can move, a commit cannot
PATCH="$PWD/scripts/eg2/llama.cpp-no-logits.patch"
VENDOR=src-tauri/helpers/fndr-eg2/vendor
SOURCE="${LLAMA_SOURCE:-$VENDOR/llama.cpp}"
BUILD="${LLAMA_BUILD:-$VENDOR/llama-build}"
OUT=$VENDOR/llama
OS=$(uname -s)
FLAVOR=$([[ $OS == Darwin ]] && echo metal || echo cpu)
STAMP="$TAG $( (shasum -a 256 "$PATCH" 2>/dev/null || sha256sum "$PATCH") | cut -c1-12) $FLAVOR"
if [[ -f "$OUT/lib/libllamacpp.a" && "$(cat "$OUT/BUILD" 2>/dev/null)" == "$STAMP" ]]; then
  exit 0
fi
if [[ -z "${DEVELOPER_DIR:-}" && -d /Applications/Xcode.app ]]; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi
if [[ ! -d "$SOURCE" ]]; then
  git -c advice.detachedHead=false -c core.autocrlf=false clone --quiet --depth 1 --branch "$TAG" \
    https://github.com/ggml-org/llama.cpp "$SOURCE"
elif [[ "$(git -C "$SOURCE" describe --tags --exact-match 2>/dev/null)" != "$TAG" ]]; then
  git -C "$SOURCE" fetch --quiet --depth 1 origin tag "$TAG"
  git -C "$SOURCE" checkout --quiet --force "$TAG"
fi
if [[ "$(git -C "$SOURCE" rev-parse HEAD)" != "$COMMIT" ]]; then
  echo "llama.cpp's $TAG is not commit $COMMIT here: not building it" >&2
  exit 1
fi
# Without the patch, every input token costs 1 MiB of logits an embedding model never makes (262k vocabulary).
if git -C "$SOURCE" apply --check "$PATCH" 2>/dev/null; then
  git -C "$SOURCE" apply "$PATCH"
elif ! git -C "$SOURCE" apply --reverse --check "$PATCH" 2>/dev/null; then
  echo "$PATCH does not apply to $SOURCE and is not applied already: not building it" >&2
  exit 1
fi
GENERATOR=$(command -v ninja > /dev/null && echo Ninja || echo "Unix Makefiles")
COMMON=(-DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DGGML_NATIVE=OFF -DLLAMA_BUILD_MTMD=OFF
  -DLLAMA_BUILD_COMMON=OFF -DLLAMA_BUILD_TOOLS=OFF -DLLAMA_BUILD_SERVER=OFF -DLLAMA_BUILD_EXAMPLES=OFF
  -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_APP=OFF -DLLAMA_BUILD_UI=OFF -DLLAMA_OPENSSL=OFF -DLLAMA_CURL=OFF)
if [[ $OS == Darwin ]]; then
  # Metal shaders embedded and compiled on first use: no Metal toolchain needed.
  cmake -S "$SOURCE" -B "$BUILD" -G "$GENERATOR" "${COMMON[@]}" -DCMAKE_OSX_DEPLOYMENT_TARGET=14.0 \
    -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON > /dev/null
  JOBS=$(sysctl -n hw.ncpu)
else
  cmake -S "$SOURCE" -B "$BUILD" -G "$GENERATOR" "${COMMON[@]}" -DGGML_OPENMP=OFF > /dev/null
  JOBS=$(nproc)
fi
cmake --build "$BUILD" --config Release -j "${CMAKE_BUILD_PARALLEL_LEVEL:-$JOBS}" --target llama > /dev/null
mkdir -p "$OUT/lib" "$OUT/include"
LIBS=$(find "$BUILD" -name '*.a' | grep -E '/(libllama|libggml[^/]*|libvendor-hash)\.a$' | sort)
rm -f "$OUT/lib/libllamacpp.a"
if [[ $OS == Darwin ]]; then
  xcrun libtool -static -no_warning_for_no_symbols -o "$OUT/lib/libllamacpp.a" $LIBS
else
  # A thin archive's members, merged into one plain archive.
  printf 'create %s\n%s\nsave\nend\n' "$OUT/lib/libllamacpp.a" "$(for l in $LIBS; do echo "addlib $l"; done)" | ar -M
fi
cp "$SOURCE/include/llama.h" "$SOURCE"/ggml/include/*.h \
  "$OUT/include/"
echo "$STAMP" > "$OUT/BUILD"
echo "Built llama.cpp $TAG (patched, $FLAVOR) into $OUT"
