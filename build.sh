#!/usr/bin/env bash

set -e

export LIBCLANG_PATH=/opt/homebrew/opt/llvm/lib
export DYLD_LIBRARY_PATH=/opt/homebrew/opt/llvm/lib:$DYLD_LIBRARY_PATH
export RUSTFLAGS="-L /opt/homebrew/lib -l sqlite3"
cargo build --features static --no-default-features
