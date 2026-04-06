# Proposal: resilience-comp-lake

**Status**: Draft
**Location**: `api_projects/resilience_comp_lake`
**Language**: Rust
**Date**: 2026-04-06

---

## Problem

Resilience testing teams have no way to measure how their testing aligns with compliance frameworks (DORA, ISO 27001, PCI DSS, NIST, CRA, GDPR, NIS2). Compliance officers rely on manual audits. There is no single source of truth for:

- Which regulatory controls are relevant to resilience testing
- How controls map across frameworks (one experiment can satisfy DORA, ISO, and PCI simultaneously)
- Whether testing evidence is fresh or stale
- What a team's compliance posture looks like over time

Existing tools (tumult, chaostooling) each carry their own partial compliance models. These diverge, can't cross-reference frameworks, and don't support multi-level scoring.

## Solution

A standalone compliance data lake that:

1. **Harvests** compliance frameworks from authoritative APIs (EUR-Lex CELLAR, NIST OSCAL, NVD, OpenSSF Scorecard) on tiered schedules
2. **Normalises** all frameworks into a unified control taxonomy with cross-framework mappings, using DORA as the EU pivot and NIST 800-53 as the global pivot
3. **Stores** everything in DuckDB + Parquet — queryable via SQL views, exportable as Arrow/Parquet for Rust consumers
4. **Scores** compliance at every granularity: experiment, project, team, unit, platform, framework — with evidence freshness decay and badge tiers
5. **Serves** via REST API, MCP server (for LLM/AQE orchestration), and DuckDB file distribution (for embedded consumers)

## Consumers

| Consumer | How they use it |
|----------|----------------|
| **Testing teams** | Dashboard with scores, badge progression ("12% to Gold on DORA"), prioritised gap list ("test next: DB failover for DORA Art.25") |
| **tumult** (Rust) | Attach DuckDB file or read Parquet — borrows control IDs, scoring weights, framework definitions. Writes evidence back via API. Replaces built-in `RegulatoryMapping` and `ResilienceScore` with comp-lake as authority. |
| **chaostooling** (Python) | REST API for control lookups and evidence submission. Replaces bespoke regulatory fields. |
| **LLM / AQE** | MCP tools: `get_compliance_score`, `get_coverage_gaps`, `recommend_experiments`, `explain_control` |
| **Compliance officers** | Audit-ready reports per framework, evidence trails per control, cross-framework coverage matrix |

## Scope

### In scope (initial)

- Frameworks: DORA, ISO 27001:2022, PCI DSS 4.0, NIST 800-53 rev5, NIST CSF 2.0, CRA, NIS2, GDPR (testing-relevant articles only)
- Harvesters: EUR-Lex CELLAR (SPARQL), NIST OSCAL (GitHub), NVD API, OpenSSF Scorecard API
- Storage: DuckDB + Parquet
- API: REST (axum) + MCP (rust-mcp-sdk)
- Scoring: weighted, multi-level, cross-framework with freshness decay
- Badges: Bronze/Silver/Gold/Platinum per framework + cross-framework badges
- Container: single static binary, distroless image (~25MB)

### Out of scope (future)

- Full GDPR text (only testing-relevant Art. 32, 35)
- SOC 2 (US-centric, add later if needed)
- Real-time streaming (Flink) — quarterly+ harvests don't need this
- UI/dashboard (consumers build their own, comp-lake provides the data)
- Tumult/chaostooling integration code (those projects adapt to comp-lake's API)

## Why Rust

- Container-optimised: ~25MB distroless image, <50ms startup, ~15MB idle RAM
- DuckDB + Arrow + Parquet crates already proven in tumult workspace
- MCP SDK (rust-mcp-sdk 0.9) already proven in tumult-mcp
- Type safety for scoring models (enums for Confidence, Trend, Severity)
- Single static binary distribution — no runtime dependencies
- Python consumers don't need the lake to be Python — they use REST or DuckDB file

## Success Criteria

- All 8 frameworks harvested and normalised with cross-framework mappings
- Scoring produces consistent results at all granularity levels
- DuckDB file can be attached by tumult and queried without the API running
- REST API + MCP server run in <25MB container
- Evidence freshness decay produces score changes without manual intervention
- A tumult GameDay journal can be ingested as evidence and produce per-control scores
