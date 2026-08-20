#!/usr/bin/env bash
# Compare Python and Rust card database / collection output.
# Run from the mtga-tool/ directory.
#
# Usage:
#   bash tests/compare/run.sh          # card-db only
#   bash tests/compare/run.sh --all    # card-db + collection (needs MTGA running)

set -euo pipefail
cd "$(dirname "$0")/../.."

echo "=== Card Database Comparison ==="
echo ""

echo "[1/3] Exporting Python card DB..."
python tests/compare/export_python.py card-db
echo ""

echo "[2/3] Exporting Rust card DB..."
cargo run --bin compare --manifest-path src-tauri/Cargo.toml -- card-db
echo ""

echo "[3/3] Diffing..."
python tests/compare/diff.py card-db
echo ""

if [[ "${1:-}" == "--all" ]]; then
    echo ""
    echo "=== Collection Comparison ==="
    echo "(Requires MTGA.exe to be running)"
    echo ""

    echo "[1/3] Exporting Python collection..."
    python tests/compare/export_python.py collection
    echo ""

    echo "[2/3] Exporting Rust collection..."
    cargo run --bin compare --manifest-path src-tauri/Cargo.toml -- collection
    echo ""

    echo "[3/3] Diffing..."
    python tests/compare/diff.py collection
    echo ""
fi
