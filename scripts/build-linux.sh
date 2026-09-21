#!/usr/bin/env bash
# Release build for Linux. No external SQLite or GUI libraries are needed:
# rusqlite is compiled in ("bundled") and eframe ships its own rendering stack.
set -euo pipefail

cd "$(dirname "$0")/.."
cargo build --release

echo
echo "Built: $(pwd)/target/release/ai-mentor"
echo "Run it with: ./target/release/ai-mentor"
