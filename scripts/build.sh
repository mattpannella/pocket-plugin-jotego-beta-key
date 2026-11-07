#!/usr/bin/env sh
# Builds target/wasm32-wasip1/release/plugin.wasm (needs: rustup target add wasm32-wasip1).
set -e
cd "$(dirname "$0")/.."
cargo build --release --target wasm32-wasip1
ls -l target/wasm32-wasip1/release/plugin.wasm
