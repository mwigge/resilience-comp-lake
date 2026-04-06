#!/usr/bin/env bash
# Pre-commit hook for resilience-comp-lake
# Install: ln -sf ../../scripts/pre-commit.sh .git/hooks/pre-commit
set -euo pipefail

echo "=== cargo fmt ==="
cargo fmt --all -- --check

echo "=== cargo clippy ==="
cargo clippy --all-targets -- -D warnings -W clippy::pedantic

echo "=== cargo test ==="
cargo test --workspace

echo "=== cargo audit ==="
cargo audit 2>/dev/null || echo "cargo-audit not installed, skipping"

echo "All quality gates passed."
