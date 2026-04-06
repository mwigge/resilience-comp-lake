# Compliance Data Lake — multi-stage build
#
# Build:
#   docker build -f docker/Dockerfile -t comp-lake .
#
# Run:
#   docker run -p 8080:8080 comp-lake serve
#   docker run comp-lake score
#   docker run comp-lake --help

FROM rust:1.89-bookworm AS builder

RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends musl-tools && \
    rustup target add x86_64-unknown-linux-musl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY comp-lake-core/ comp-lake-core/
COPY comp-lake-storage/ comp-lake-storage/
COPY comp-lake-harvesters/ comp-lake-harvesters/
COPY comp-lake-api/ comp-lake-api/
COPY comp-lake-cli/ comp-lake-cli/

RUN cargo build --release --target x86_64-unknown-linux-musl -p comp-lake-cli && \
    strip target/x86_64-unknown-linux-musl/release/comp-lake

# ── Runtime ─────────────────────────────────────────────────

FROM gcr.io/distroless/static-debian12:nonroot

COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/comp-lake /usr/local/bin/
COPY data/seed/ /opt/comp-lake/data/seed/

ENV COMP_LAKE_DATA_DIR=/opt/comp-lake/data

EXPOSE 8080

HEALTHCHECK --interval=10s --timeout=5s --retries=3 \
    CMD ["/usr/local/bin/comp-lake", "--version"]

ENTRYPOINT ["comp-lake"]
CMD ["serve"]
