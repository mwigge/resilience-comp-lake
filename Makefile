.PHONY: check fmt lint test audit build demo demo-clean docker-build docker-run clean

DB ?= /tmp/comp-lake-demo.duckdb

# Full quality gate — run before every commit
check: fmt lint test audit

fmt:
	cargo fmt --all -- --check

lint:
	cargo clippy --all-targets -- -D warnings -W clippy::pedantic

test:
	cargo test --workspace

audit:
	cargo audit

build:
	cargo build --release -p comp-lake-cli

# ── Demo ────────────────────────────────────────────────────

demo: demo-clean build-dev
	@echo "=== Loading seed frameworks and mappings ==="
	cargo run -p comp-lake-cli -- --db $(DB) seed
	@echo ""
	@echo "=== Creating demo org hierarchy and evidence ==="
	cargo run -p comp-lake-cli -- --db $(DB) demo
	@echo ""
	@echo "=== Compliance Scores ==="
	cargo run -p comp-lake-cli -- --db $(DB) score --entity proj-payments
	@echo ""
	@echo "=== Coverage Gaps (top 20) ==="
	cargo run -p comp-lake-cli -- --db $(DB) gaps --entity proj-payments
	@echo ""
	@echo "=== Database Stats ==="
	cargo run -p comp-lake-cli -- --db $(DB) stats
	@echo ""
	@echo "Demo database at: $(DB)"
	@echo "Run more queries:"
	@echo "  cargo run -p comp-lake-cli -- --db $(DB) score --entity proj-auth"
	@echo "  cargo run -p comp-lake-cli -- --db $(DB) score --entity proj-payments --framework DORA"
	@echo "  cargo run -p comp-lake-cli -- --db $(DB) gaps --entity proj-gateway"

demo-clean:
	@rm -f $(DB)

build-dev:
	@cargo build -p comp-lake-cli 2>/dev/null

# ── Docker ──────────────────────────────────────────────────

docker-build:
	docker build -f docker/Dockerfile -t comp-lake .

docker-run:
	docker run -p 8080:8080 comp-lake serve

clean:
	cargo clean
	rm -f $(DB)
