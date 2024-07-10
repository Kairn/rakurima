#!/usr/bin/env bash

set -euo pipefail

# Run Clippy
cargo clippy --manifest-path ./rakurima/Cargo.toml -- -Dwarnings

# Format code
cargo fmt --manifest-path ./rakurima/Cargo.toml

exit 0
