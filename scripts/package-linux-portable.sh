#!/usr/bin/env bash
# Reuses the local toolchain and sources, but links against Ubuntu 24.04 libc.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD="$ROOT/dist/build-linux"
TOOLCHAIN="$(dirname "$(dirname "$(rustup which rustc)")")"
[[ "$("$TOOLCHAIN/bin/rustc" --version)" == 'rustc 1.93.0 '* ]] || {
    echo 'La compilación de entrega está fijada a Rust 1.93.0.' >&2; exit 1;
}
mkdir -p "$BUILD/cargo" "$ROOT/dist/portable"
# Registry sources may be written by dependency build scripts. Never mount the
# user's registry writable into the build container.
cp -a -n "${CARGO_HOME:-$HOME/.cargo}/registry" "$BUILD/cargo/"
ORT_DIR="${QUIRON_ORT_LIB_DIR:-$HOME/.cache/ort.pyke.io/dfbin/x86_64-unknown-linux-gnu/8c57d059aaaee407812a5698d6706c79e090ad69e1a14204309e802dcbbaa35f}"
[[ -f "$ORT_DIR/libonnxruntime.a" ]] || {
    echo 'Falta la dependencia ONNX Runtime local; compilar primero quiron-brain con las versiones de Cargo.lock.' >&2; exit 1;
}
docker build -f "$ROOT/deploy/Dockerfile.build-linux" -t quiron-build:ubuntu24.04 "$ROOT/deploy"
BUILD_IMAGE="$(docker image inspect quiron-build:ubuntu24.04 --format '{{.Id}}')"
docker run --rm --network none --user "$(id -u):$(id -g)" \
    --memory 6g --memory-swap 6g --cpus 2 \
    --mount "type=bind,source=$ROOT,target=/src,readonly" \
    --mount "type=bind,source=$BUILD,target=/build" \
    --mount "type=bind,source=$ROOT/dist/portable,target=/out" \
    --mount "type=bind,source=$TOOLCHAIN,target=/opt/rust,readonly" \
    --mount "type=bind,source=$ORT_DIR,target=/opt/ort,readonly" \
    -e CARGO_HOME=/build/cargo -e CARGO_NET_OFFLINE=true -e CARGO_BUILD_JOBS=1 \
    -e CC=gcc-13 -e CXX=g++-13 \
    -e ORT_LIB_LOCATION=/opt/ort -e QUIRON_BUILD_DIR=/build/targets \
    -e QUIRON_PACKAGE_DIR=/out -e QUIRON_BUILD_IMAGE="$BUILD_IMAGE" \
    -e QUIRON_GLIBC_CEILING=2.39 -e XDG_CONFIG_HOME=/build/config \
    --workdir /src "$BUILD_IMAGE" bash scripts/package-linux.sh
