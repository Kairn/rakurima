#!/usr/bin/env bash

set -euo pipefail

# Remove Maelstrom run history
rm -rf ./store

# Clean up Rust build
cargo clean --manifest-path ./rakurima/Cargo.toml

exit 0
