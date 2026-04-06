# resilience-comp-lake

Standalone compliance data lake for resilience testing scoring.

Harvests regulatory frameworks (DORA, ISO 27001, PCI DSS, NIST 800-53/CSF, CRA, NIS2, GDPR), normalises controls into a unified taxonomy with cross-framework mappings, computes multi-level compliance scores with evidence freshness decay, and serves via REST API, MCP server, and DuckDB file distribution.

## Quick Start

```bash
# Build and run the full demo (seed data + org hierarchy + evidence + scores)
make demo
```

This creates a DuckDB database at `/tmp/comp-lake-demo.duckdb` with:
- 2 frameworks (DORA, ISO 27001) with 51 controls
- 20 cross-framework mappings (DORA ↔ ISO 27001)
- 7 org entities (platform → unit → teams → projects)
- 30 evidence records (chaos experiments + GameDay results)

Example output:

```
Compliance scores for entity: proj-payments

Framework             Score    Total     Pass  Stale      Badge
--------------------------------------------------------------
DORA                  36.8%       19        7      0       None
ISO-27001-2022         0.0%       30        0      0       None
```

## CLI Commands

```bash
# Load seed frameworks and mappings
comp-lake --db demo.duckdb seed

# Create demo data (org hierarchy + sample evidence)
comp-lake --db demo.duckdb demo

# Show compliance scores
comp-lake --db demo.duckdb score --entity proj-payments
comp-lake --db demo.duckdb score --entity proj-payments --framework DORA

# Show prioritised coverage gaps
comp-lake --db demo.duckdb gaps --entity proj-payments

# Database statistics
comp-lake --db demo.duckdb stats
```

## Architecture

```
comp-lake-core        — Domain models, scoring engine, freshness rules
comp-lake-storage     — DuckDB schema, Arrow schemas, Parquet export, analytical views
comp-lake-harvesters  — EUR-Lex CELLAR, NIST OSCAL, NVD, OpenSSF Scorecard, seed loader
comp-lake-api         — REST (axum) + MCP server (planned)
comp-lake-cli         — CLI entry point
```

- **Language**: Rust
- **Storage**: DuckDB + Parquet (Arrow columnar format)
- **API**: REST (axum) + MCP (rust-mcp-sdk)
- **Container**: distroless static binary (~25MB image)
- **Tests**: 164 unit tests across all crates

## Scoring Model

- Only controls marked `testing_relevant = true` count toward scores
- Evidence expires based on type (chaos experiment: 90d, GameDay: 180d, pen test: 365d)
- Stale evidence does not count — scores degrade automatically over time
- Cross-framework mappings allow evidence to satisfy multiple frameworks simultaneously
- Badge tiers: None (<50%), Bronze (≥50%), Silver (≥70%), Gold (≥85%), Platinum (≥95%)

## Frameworks

| Framework | Region | Pivot | Controls |
|-----------|--------|-------|----------|
| DORA (2022/2554) | EU | Primary (EU) | ~20 testing-relevant |
| ISO 27001:2022 | Global | Bridge (EU ↔ Global) | ~30 testing-relevant |
| PCI DSS 4.0.1 | Global | — | ~60 testing-relevant |
| NIST 800-53 rev5 | US/Global | Secondary (Global) | ~150 testing-relevant |
| NIST CSF 2.0 | US/Global | — | ~40 testing-relevant |
| CRA (2024/2847) | EU | — | ~15 testing-relevant |
| NIS2 (2022/2555) | EU | — | ~20 testing-relevant |
| GDPR (2016/679) | EU | Art. 32, 35 only | ~5 testing-relevant |

## Cross-Framework Mappings

44 seed mappings across 4 framework pairs:

| Pair | Mappings | Provenance |
|------|----------|------------|
| DORA ↔ ISO 27001 | 20 | EBA guidelines + manual |
| DORA ↔ NIS2 | 12 | Same EU legislative package |
| DORA ↔ CRA | 8 | Manual analysis |
| DORA ↔ GDPR | 4 | Manual (narrow scope) |

## Consumers

| Consumer | Integration |
|----------|-------------|
| Testing teams | CLI + REST API + dashboard |
| tumult / chaostooling | DuckDB file attach or Parquet read |
| LLM / AQE | MCP tools |
| Compliance officers | REST API + report exports |

## Development

```bash
# Run quality gate
make check

# Run tests only
make test

# Build release binary
make build

# Docker
make docker-build
make docker-run
```

## License

Apache-2.0
