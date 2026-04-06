# resilience-comp-lake

Standalone compliance data lake for resilience testing. Maps chaos engineering experiments, security scans, and operational metrics to regulatory framework requirements — producing multi-level compliance scores with evidence freshness decay and cross-framework mappings.

One chaos experiment can satisfy DORA, ISO 27001, and NIST 800-53 simultaneously.

## Why

Resilience testing teams have no way to measure how their testing aligns with compliance frameworks. Compliance officers rely on manual audits. This project provides:

- **Unified control taxonomy** across 8 regulatory frameworks
- **Cross-framework mappings** — one experiment satisfies multiple frameworks via the dual-pivot model
- **Evidence-based scoring** with automatic freshness decay (stale tests reduce scores without intervention)
- **Prioritised gap analysis** — "test next" lists ranked by severity and coverage
- **DuckDB + Parquet** storage — queryable SQL views, exportable for embedded consumers

## Quick Start

```bash
# Build and run the full demo
make demo
```

Output:
```
Compliance scores for entity: proj-payments

Framework             Score    Total     Pass  Stale      Badge
--------------------------------------------------------------
DORA                  36.8%       19        7      0       None
ISO-27001-2022         0.0%       30        0      0       None
```

## CLI

```bash
comp-lake --db demo.duckdb seed                               # Load frameworks + mappings
comp-lake --db demo.duckdb demo                               # Create sample org + evidence
comp-lake --db demo.duckdb score --entity proj-payments        # Compliance scores
comp-lake --db demo.duckdb score --entity proj-payments --framework DORA
comp-lake --db demo.duckdb gaps --entity proj-payments         # Prioritised coverage gaps
comp-lake --db demo.duckdb stats                               # Database statistics
```

## Frameworks

8 regulatory frameworks with 44 cross-framework mappings:

| Framework | Region | Role | Controls | Harvest Source |
|-----------|--------|------|----------|----------------|
| DORA (2022/2554) | EU | EU Pivot | ~20 | EUR-Lex CELLAR SPARQL |
| ISO 27001:2022 | Global | Bridge | ~30 | Seed (TOML) |
| PCI DSS 4.0.1 | Global | — | ~60 | Seed (TOML) |
| NIST 800-53 Rev.5 | US/Global | Global Pivot | ~150 | OSCAL (GitHub JSON) |
| NIST CSF 2.0 | US/Global | — | ~40 | OSCAL (GitHub JSON) |
| CRA (2024/2847) | EU | — | ~15 | EUR-Lex CELLAR SPARQL |
| NIS2 (2022/2555) | EU | — | ~20 | EUR-Lex CELLAR SPARQL |
| GDPR (2016/679) | EU | Art. 32, 35 | ~5 | EUR-Lex CELLAR SPARQL |

## Cross-Framework Dual-Pivot Model

```
                    DORA (EU Pivot)
                   /    |    \    \
                NIS2   CRA  GDPR  ISO 27001 (Bridge)
                                     |
                              NIST 800-53 (Global Pivot)
                               /         \
                           CSF 2.0    PCI DSS 4.0
```

Evidence propagates across frameworks via control mappings. A vulnerability scan satisfying ISO A.8.8 automatically credits DORA Art.25 through the DORA ↔ ISO mapping. See [Cross-Framework Mappings](docs/cross-framework-mappings.md) for details.

## Scoring Model

- Only `testing_relevant` controls count (governance/documentation excluded)
- Evidence expires by type: chaos experiments (90d), GameDays (180d), pen tests (365d), vuln scans (30d)
- Badge tiers: None (<50%), Bronze (>=50%), Silver (>=70%), Gold (>=85%), Platinum (>=95%)
- Cross-framework enhancement: mapped evidence fills gaps (confidence >= Medium required)
- Hierarchical roll-up: Project → Team → Unit → Platform (arithmetic mean)
- Trend detection: Improving (>=+5), Stable, Degrading (<=-5) over 30 days

See [Scoring Model](docs/scoring.md) for the full methodology.

## Architecture

```
comp-lake-core        Models, scoring engine, freshness rules, config
comp-lake-storage     DuckDB schema, Arrow schemas, analytical views, Parquet export
comp-lake-harvesters  EUR-Lex CELLAR, NIST OSCAL, NVD, OpenSSF Scorecard, seed loader
comp-lake-api         REST (axum) + MCP server (planned)
comp-lake-cli         CLI entry point
```

| Concern | Technology |
|---------|-----------|
| Language | Rust (1.89+) |
| Storage | DuckDB + Parquet (Arrow columnar) |
| API | REST (axum) + MCP (rust-mcp-sdk) |
| Container | distroless static binary (~25MB) |
| Tests | 168 unit + integration tests |

## DuckDB Analytical Views

The views are the core contract for embedded consumers:

| View | Purpose |
|------|---------|
| `v_scores` | Per-entity, per-framework compliance score with badge |
| `v_rollup_scores` | Recursive CTE: project → team → unit → platform |
| `v_coverage_gaps` | Prioritised untested/stale/failing controls |
| `v_cross_framework_map` | Control-to-control mappings across frameworks |
| `v_evidence_freshness` | Fresh vs stale evidence counts per entity |

## Consumers

| Consumer | Integration |
|----------|-------------|
| Testing teams | CLI + REST API + DuckDB views |
| [tumult](https://github.com/mwigge/tumult) | DuckDB file attach or Parquet read |
| [chaostooling-oss](https://github.com/mwigge/chaostooling-oss) | REST API for evidence submission |
| LLM / AQE agents | MCP tools (planned) |
| Compliance officers | REST API + Parquet exports |

## Documentation

| Document | Description |
|----------|-------------|
| [Scoring Model](docs/scoring.md) | Formula, evidence types, freshness, badges, roll-up |
| [Frameworks](docs/frameworks.md) | All 8 frameworks, harvest sources, cadences |
| [Cross-Framework Mappings](docs/cross-framework-mappings.md) | Dual-pivot model, mapping tables, examples |

## Development

```bash
make check          # Full quality gate (fmt + clippy + test + audit)
make test           # Tests only
make demo           # End-to-end demo
make build          # Release binary
make docker-build   # Docker image
```

## License

Apache-2.0
