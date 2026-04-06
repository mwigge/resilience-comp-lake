# resilience-comp-lake

Standalone compliance data lake for resilience testing scoring.

Harvests regulatory frameworks (DORA, ISO 27001, PCI DSS, NIST 800-53/CSF, CRA, NIS2, GDPR), normalises controls into a unified taxonomy, computes multi-level compliance scores, and serves via REST API, MCP server, and DuckDB file distribution.

## Status

Early development. See `docs/` for proposal, design, methodology, and task breakdown.

## Architecture

- **Language**: Rust
- **Storage**: DuckDB + Parquet (Arrow columnar format)
- **API**: REST (axum) + MCP (rust-mcp-sdk)
- **Container**: distroless static binary (~25MB image)

## Consumers

| Consumer | Integration |
|----------|-------------|
| Testing teams | REST API + dashboard |
| tumult / chaostooling | DuckDB file attach or Parquet read |
| LLM / AQE | MCP tools |
| Compliance officers | REST API + report exports |

## Frameworks

| Framework | Region | Pivot |
|-----------|--------|-------|
| DORA (2022/2554) | EU | Primary (EU) |
| ISO 27001:2022 | Global | Bridge (EU ↔ Global) |
| PCI DSS 4.0.1 | Global | - |
| NIST 800-53 rev5 | US/Global | Secondary (Global) |
| NIST CSF 2.0 | US/Global | - |
| CRA (2024/2847) | EU | - |
| NIS2 (2022/2555) | EU | - |
| GDPR (2016/679) | EU | Testing-relevant articles only |

## License

Apache-2.0
