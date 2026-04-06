.PHONY: check fmt lint test audit build docker-build docker-run clean

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

docker-build:
	docker build -f docker/Dockerfile -t comp-lake .

docker-run:
	docker run -p 8080:8080 comp-lake serve

clean:
	cargo clean
