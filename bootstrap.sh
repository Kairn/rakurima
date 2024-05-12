#!/usr/bin/env bash

set -euo pipefail

# Prepare the Maelstrom binary
if [[ ! -f ./maelstrom/maelstrom ]]; then
    echo "No Maelstrom binary detected, extracting the default tar file..."
    tar xf maelstrom.tar.bz2
else
    echo "Detected Maelstrom binary, proceeding."
fi

# Compile the test program
echo "Compiling the test program..."
cargo build --manifest-path ./rakurima/Cargo.toml
echo "Test program compiled."

echo "All set. Ready to test."

exit 0
