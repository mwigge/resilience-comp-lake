# Design: resilience-comp-lake

**Status**: Draft
**Date**: 2026-04-06

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    resilience-comp-lake (Rust)                           │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────┐      │
│  │                     HARVEST LAYER                             │      │
│  │                                                               │      │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐       │      │
│  │  │ EUR-Lex  │ │ OSCAL    │ │ NVD      │ │ OpenSSF  │       │      │
│  │  │ CELLAR   │ │ GitHub   │ │ CVE API  │ │ Scorecard│       │      │
│  │  │ (SPARQL) │ │ (JSON)   │ │ (REST)   │ │ (REST)   │       │      │
│  │  └────┬─────┘ └────┬─────┘ └────┬─────┘ └────┬─────┘       │      │
│  │       │  Harvester trait: async fn harvest() -> HarvestResult│      │
│  └───────┼──────────────┼──────────────┼──────────────┼─────────┘      │
│          │              │              │              │                 │
│          ▼              ▼              ▼              ▼                 │
│  ┌──────────────────────────────────────────────────────────────┐      │
│  │                   NORMALISATION LAYER                         │      │
│  │                                                               │      │
│  │  Raw harvest → Framework + Controls + ControlMappings         │      │
│  │  Each harvester produces typed structs, normaliser writes     │      │
│  │  to DuckDB tables and Parquet snapshots                       │      │
│  └──────────────────────────┬───────────────────────────────────┘      │
│                              │                                          │
│                              ▼                                          │
│  ┌──────────────────────────────────────────────────────────────┐      │
│  │                     STORAGE LAYER                             │      │
│  │                                                               │      │
│  │  DuckDB (comp_lake.duckdb)                                   │      │
│  │  ├── Tables: frameworks, controls, control_mappings,         │      │
│  │  │           org_hierarchy, evidence, harvest_log             │      │
│  │  ├── Views:  v_scores, v_team_scores, v_platform_scores,    │      │
│  │  │           v_coverage_gaps, v_cross_framework_map,         │      │
│  │  │           v_badge_transitions, v_evidence_freshness       │      │
│  │  └── Schema versioning via schema_meta table                 │      │
│  │                                                               │      │
│  │  Parquet snapshots (data/frameworks/, data/evidence/)        │      │
│  │  └── Versioned by harvest date for time-travel               │      │
│  └──────────────────────────┬───────────────────────────────────┘      │
│                              │                                          │
│          ┌──────────────────┼───────────────────┐                      │
│          ▼                  ▼                   ▼                       │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────┐               │
│  │  REST API    │  │  MCP Server  │  │  CLI           │               │
│  │  (axum)      │  │  (rust-mcp)  │  │  (clap)        │               │
│  │              │  │              │  │                │               │
│  │  /scores     │  │  tools:      │  │  harvest       │               │
│  │  /controls   │  │  get_score   │  │  score         │               │
│  │  /evidence   │  │  get_gaps    │  │  export        │               │
│  │  /gaps       │  │  recommend   │  │  serve         │               │
│  │  /frameworks │  │  explain     │  │  validate      │               │
│  │  /harvest    │  │              │  │                │               │
│  └──────────────┘  └──────────────┘  └────────────────┘               │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

## Crate Structure

```
api_projects/resilience_comp_lake/
├── Cargo.toml                  (workspace root)
├── comp-lake-core/             (models, types, scoring logic)
│   └── src/
│       ├── lib.rs
│       ├── models/
│       │   ├── framework.rs    Framework, Control, ControlMapping
│       │   ├── evidence.rs     Evidence, EvidenceType, FreshnessRule
│       │   ├── org.rs          OrgEntity, EntityType (hierarchy)
│       │   ├── scoring.rs      ComplianceScore, Badge, BadgeTier, Trend
│       │   └── harvest.rs      HarvestResult, HarvestLog
│       ├── scoring/
│       │   ├── engine.rs       score computation, roll-up logic
│       │   ├── weights.rs      per-framework weight configuration
│       │   ├── freshness.rs    evidence expiry rules per type
│       │   └── badges.rs       tier thresholds, cross-framework badges
│       └── config.rs           framework registry, scoring config
│
├── comp-lake-harvesters/       (framework data acquisition)
│   └── src/
│       ├── lib.rs
│       ├── harvester.rs        Harvester trait definition
│       ├── cellar.rs           EUR-Lex SPARQL (DORA, CRA, GDPR, NIS2)
│       ├── oscal.rs            NIST OSCAL GitHub (800-53, CSF)
│       ├── nvd.rs              NIST NVD CVE API
│       ├── scorecard.rs        OpenSSF Scorecard API
│       └── manual.rs           Manual/CSV import (PCI DSS, ISO 27001)
│
├── comp-lake-storage/          (DuckDB + Parquet persistence)
│   └── src/
│       ├── lib.rs
│       ├── duckdb.rs           schema creation, migrations, queries
│       ├── arrow.rs            Arrow schema definitions
│       ├── views.rs            SQL view definitions (the core contract)
│       ├── export.rs           Parquet/Arrow IPC/CSV export
│       └── ingest.rs           evidence ingestion from any source
│
├── comp-lake-api/              (REST + MCP servers)
│   └── src/
│       ├── lib.rs
│       ├── rest/
│       │   ├── app.rs          axum router setup
│       │   ├── scores.rs       GET /api/v1/scores/{entity}
│       │   ├── controls.rs     GET /api/v1/controls
│       │   ├── evidence.rs     POST /api/v1/evidence
│       │   ├── gaps.rs         GET /api/v1/gaps/{entity}
│       │   ├── frameworks.rs   GET /api/v1/frameworks
│       │   └── harvest.rs      POST /api/v1/harvest/trigger
│       └── mcp/
│           └── server.rs       MCP tools + resources
│
├── comp-lake-cli/              (binary entry point)
│   └── src/
│       └── main.rs             clap CLI: harvest, score, export, serve
│
├── data/
│   ├── seed/                   initial framework data (DORA articles etc.)
│   ├── frameworks/             harvested snapshots (Parquet)
│   └── evidence/               evidence snapshots (Parquet)
│
├── docker/
│   └── Dockerfile              multi-stage → distroless static binary
│
└── docs/
    └── methodology.md          scoring methodology (required before impl)
```

## Data Model

### Core Tables

```sql
-- Schema version tracking (same pattern as tumult-analytics)
CREATE TABLE schema_meta (
    key   VARCHAR PRIMARY KEY,
    value VARCHAR NOT NULL
);

-- Compliance frameworks
CREATE TABLE frameworks (
    framework_id   VARCHAR PRIMARY KEY,  -- 'dora', 'iso_27001_2022'
    name           VARCHAR NOT NULL,     -- 'Digital Operational Resilience Act'
    version        VARCHAR NOT NULL,     -- '2022/2554'
    region         VARCHAR NOT NULL,     -- 'EU', 'global', 'US'
    authority      VARCHAR NOT NULL,     -- 'EU/EP', 'ISO/IEC', 'NIST'
    is_pivot       BOOLEAN NOT NULL DEFAULT false,
    effective_date DATE,
    sunset_date    DATE,
    celex_id       VARCHAR,             -- EU only: '32022R2554'
    eli_uri        VARCHAR,             -- EU only: 'http://data.europa.eu/eli/...'
    harvest_source VARCHAR NOT NULL,     -- 'cellar_sparql', 'oscal_github'
    last_harvested TIMESTAMP
);

-- Individual controls / articles / requirements
CREATE TABLE controls (
    control_id       VARCHAR PRIMARY KEY,  -- 'DORA-25.1', 'CP-4', 'A.17.1.3'
    framework_id     VARCHAR NOT NULL REFERENCES frameworks,
    article_ref      VARCHAR,              -- 'Art. 25(1)' (EU style)
    chapter_ref      VARCHAR,              -- 'Chapter IV'
    title            VARCHAR NOT NULL,
    description      TEXT,
    family           VARCHAR NOT NULL,     -- 'Resilience Testing', 'Risk Management'
    severity         VARCHAR NOT NULL,     -- 'HIGH', 'MODERATE', 'LOW'
    testing_relevant BOOLEAN NOT NULL DEFAULT false,
    parent_id        VARCHAR REFERENCES controls
);

-- Cross-framework control mappings
CREATE TABLE control_mappings (
    source_control VARCHAR NOT NULL REFERENCES controls,
    target_control VARCHAR NOT NULL REFERENCES controls,
    relationship   VARCHAR NOT NULL,  -- 'equivalent', 'partial', 'supplements'
    confidence     VARCHAR NOT NULL,  -- 'low', 'medium', 'high'
    direction      VARCHAR NOT NULL DEFAULT 'bidirectional',
    provenance     VARCHAR NOT NULL,  -- 'eba_mapping', 'nist_olir', 'manual'
    PRIMARY KEY (source_control, target_control)
);

-- Organisational hierarchy (project < team < unit < platform)
CREATE TABLE org_hierarchy (
    entity_id   VARCHAR PRIMARY KEY,
    entity_type VARCHAR NOT NULL,  -- 'platform', 'unit', 'team', 'project'
    name        VARCHAR NOT NULL,
    parent_id   VARCHAR REFERENCES org_hierarchy
);

-- Evidence records (from consumers: tumult, chaostooling, scanners, etc.)
CREATE TABLE evidence (
    evidence_id   VARCHAR PRIMARY KEY,  -- UUID
    entity_id     VARCHAR NOT NULL REFERENCES org_hierarchy,
    control_id    VARCHAR NOT NULL REFERENCES controls,
    evidence_type VARCHAR NOT NULL,     -- 'chaos_experiment', 'gameday', 'pen_test', ...
    source_system VARCHAR NOT NULL,     -- 'tumult', 'chaostooling', 'sonarqube'
    result        VARCHAR NOT NULL,     -- 'pass', 'fail', 'partial'
    score         DOUBLE NOT NULL,      -- 0.0-1.0 normalised
    metadata      JSON,                 -- source-specific details
    observed_at   TIMESTAMP NOT NULL,
    expires_at    TIMESTAMP NOT NULL    -- observed_at + freshness_period
);

-- Harvest log (audit trail)
CREATE TABLE harvest_log (
    harvest_id    VARCHAR PRIMARY KEY,
    framework_id  VARCHAR NOT NULL REFERENCES frameworks,
    started_at    TIMESTAMP NOT NULL,
    completed_at  TIMESTAMP,
    status        VARCHAR NOT NULL,  -- 'running', 'completed', 'failed'
    controls_added    INTEGER DEFAULT 0,
    controls_updated  INTEGER DEFAULT 0,
    mappings_added    INTEGER DEFAULT 0,
    error_message TEXT
);
```

### Analytical Views (the core contract)

```sql
-- Per-entity, per-framework compliance score
CREATE VIEW v_scores AS
SELECT
    e.entity_id,
    e.name              AS entity_name,
    e.entity_type,
    f.framework_id,
    f.name              AS framework_name,
    COUNT(DISTINCT c.control_id)
        FILTER (WHERE c.testing_relevant)                           AS controls_total,
    COUNT(DISTINCT ev.control_id)
        FILTER (WHERE ev.result IN ('pass', 'partial')
                  AND ev.expires_at > CURRENT_TIMESTAMP)            AS controls_covered,
    COUNT(DISTINCT ev.control_id)
        FILTER (WHERE ev.result = 'pass'
                  AND ev.expires_at > CURRENT_TIMESTAMP)            AS controls_passing,
    COUNT(DISTINCT ev.control_id)
        FILTER (WHERE ev.expires_at <= CURRENT_TIMESTAMP)           AS controls_stale,
    ROUND(100.0 * COUNT(DISTINCT ev.control_id)
        FILTER (WHERE ev.result = 'pass'
                  AND ev.expires_at > CURRENT_TIMESTAMP)
        / NULLIF(COUNT(DISTINCT c.control_id)
            FILTER (WHERE c.testing_relevant), 0), 1)              AS score,
    CASE
        WHEN score >= 95 THEN 'platinum'
        WHEN score >= 85 THEN 'gold'
        WHEN score >= 70 THEN 'silver'
        WHEN score >= 50 THEN 'bronze'
        ELSE 'none'
    END AS badge
FROM org_hierarchy e
CROSS JOIN frameworks f
LEFT JOIN controls c
    ON c.framework_id = f.framework_id
   AND c.testing_relevant = true
LEFT JOIN evidence ev
    ON ev.entity_id = e.entity_id
   AND ev.control_id = c.control_id
GROUP BY e.entity_id, e.name, e.entity_type, f.framework_id, f.name;

-- Roll-up: team = avg of projects, unit = avg of teams, etc.
CREATE VIEW v_rollup_scores AS
WITH RECURSIVE rollup AS (
    -- Base: project-level scores
    SELECT entity_id, entity_type, parent_id, framework_id, score, badge
    FROM v_scores s
    JOIN org_hierarchy o USING (entity_id)
    WHERE entity_type = 'project'
    UNION ALL
    -- Recursive: parent = avg of children
    SELECT p.entity_id, p.entity_type, p.parent_id,
           r.framework_id,
           ROUND(AVG(r.score), 1) AS score,
           CASE
               WHEN AVG(r.score) >= 95 THEN 'platinum'
               WHEN AVG(r.score) >= 85 THEN 'gold'
               WHEN AVG(r.score) >= 70 THEN 'silver'
               WHEN AVG(r.score) >= 50 THEN 'bronze'
               ELSE 'none'
           END AS badge
    FROM org_hierarchy p
    JOIN rollup r ON r.parent_id = p.entity_id
    GROUP BY p.entity_id, p.entity_type, p.parent_id, r.framework_id
)
SELECT * FROM rollup;

-- Coverage gaps: what to test next, prioritised
CREATE VIEW v_coverage_gaps AS
SELECT
    e.entity_id,
    e.name              AS entity_name,
    c.control_id,
    c.title,
    c.severity,
    c.framework_id,
    f.name              AS framework_name,
    COALESCE(best_ev.result, 'not_tested') AS current_status,
    best_ev.expires_at,
    CASE
        WHEN best_ev.evidence_id IS NULL   THEN 'never_tested'
        WHEN best_ev.expires_at <= NOW()   THEN 'stale'
        WHEN best_ev.result = 'fail'       THEN 'failing'
        WHEN best_ev.result = 'partial'    THEN 'partial'
    END AS gap_reason,
    -- Priority: HIGH+never > HIGH+stale > HIGH+fail > MOD+never ...
    ROW_NUMBER() OVER (
        PARTITION BY e.entity_id
        ORDER BY
            CASE c.severity WHEN 'HIGH' THEN 1 WHEN 'MODERATE' THEN 2 ELSE 3 END,
            CASE
                WHEN best_ev.evidence_id IS NULL THEN 1
                WHEN best_ev.expires_at <= NOW() THEN 2
                WHEN best_ev.result = 'fail' THEN 3
                ELSE 4
            END
    ) AS priority_rank
FROM org_hierarchy e
CROSS JOIN controls c
JOIN frameworks f ON c.framework_id = f.framework_id
LEFT JOIN LATERAL (
    SELECT ev.*
    FROM evidence ev
    WHERE ev.entity_id = e.entity_id
      AND ev.control_id = c.control_id
    ORDER BY ev.observed_at DESC
    LIMIT 1
) best_ev ON true
WHERE c.testing_relevant = true
  AND (best_ev.evidence_id IS NULL
       OR best_ev.expires_at <= NOW()
       OR best_ev.result IN ('fail', 'partial'));

-- Cross-framework map: which controls satisfy multiple frameworks
CREATE VIEW v_cross_framework_map AS
SELECT
    c1.control_id    AS control_a,
    c1.framework_id  AS framework_a,
    c1.title         AS title_a,
    m.relationship,
    m.confidence,
    c2.control_id    AS control_b,
    c2.framework_id  AS framework_b,
    c2.title         AS title_b
FROM control_mappings m
JOIN controls c1 ON m.source_control = c1.control_id
JOIN controls c2 ON m.target_control = c2.control_id;

-- Evidence freshness dashboard
CREATE VIEW v_evidence_freshness AS
SELECT
    e.entity_id,
    e.name,
    ev.evidence_type,
    ev.source_system,
    COUNT(*) AS total_evidence,
    COUNT(*) FILTER (WHERE ev.expires_at > NOW()) AS fresh,
    COUNT(*) FILTER (WHERE ev.expires_at <= NOW()) AS stale,
    ROUND(100.0 * COUNT(*) FILTER (WHERE ev.expires_at > NOW())
        / COUNT(*), 1) AS freshness_pct
FROM org_hierarchy e
JOIN evidence ev ON ev.entity_id = e.entity_id
GROUP BY e.entity_id, e.name, ev.evidence_type, ev.source_system;
```

## Harvester Trait

```rust
/// All harvesters implement this trait.
/// Each harvester fetches from one source and produces normalised controls.
#[async_trait]
pub trait Harvester: Send + Sync {
    /// Human-readable name for logging
    fn name(&self) -> &str;

    /// Which frameworks this harvester provides
    fn frameworks(&self) -> &[FrameworkId];

    /// Execute the harvest. Returns normalised controls and mappings.
    async fn harvest(&self, config: &HarvestConfig) -> Result<HarvestResult>;

    /// Recommended cadence for scheduling
    fn cadence(&self) -> HarvestCadence;
}

pub enum HarvestCadence {
    Daily,
    Weekly,
    Monthly,
    OnRelease,   // watch GitHub releases
    OnVersion,   // manual trigger on version bump
}

pub struct HarvestResult {
    pub framework: Framework,
    pub controls: Vec<Control>,
    pub mappings: Vec<ControlMapping>,
    pub snapshot_version: String,
}
```

## Evidence Freshness Rules

```rust
pub fn freshness_period(evidence_type: &EvidenceType) -> Duration {
    match evidence_type {
        EvidenceType::ChaosExperiment => Duration::days(90),
        EvidenceType::GameDay         => Duration::days(180),
        EvidenceType::PenTest         => Duration::days(365),
        EvidenceType::VulnScan        => Duration::days(30),
        EvidenceType::DoraMetric      => Duration::days(30),
        EvidenceType::Scorecard       => Duration::days(14),
        EvidenceType::AuditFinding    => Duration::days(365),
        EvidenceType::UnitTest        => Duration::days(30),
        EvidenceType::IntegrationTest => Duration::days(60),
    }
}
```

## Scoring Engine

### Per-Entity, Per-Framework Score

```
score = (controls_passing_with_fresh_evidence / controls_total_testing_relevant) * 100
```

Simple ratio. No weights needed at this level — the weight is implicit in which controls are marked `testing_relevant` and their severity.

### Cross-Framework Score

When one experiment satisfies controls in multiple frameworks (via control_mappings), the evidence propagates. A pass on NIST CP-4 automatically counts for DORA Art.25.1 if the mapping exists with sufficient confidence.

```
cross_score(entity, framework_A) =
    direct_score(entity, framework_A)
    + Σ mapped_score(entity, framework_B → A) * mapping_confidence
```

Capped at 100. Mapped evidence contributes proportionally to mapping confidence (high=1.0, medium=0.7, low=0.4).

### Roll-Up Score

```
team_score     = avg(project_scores)
unit_score     = avg(team_scores)
platform_score = avg(unit_scores)
```

Simple average. Can be weighted by project criticality later if needed.

## Container Strategy

```dockerfile
# Build stage
FROM rust:1.89-slim AS builder
WORKDIR /build
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl

# Runtime stage
FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/comp-lake /
COPY data/seed/ /data/seed/
EXPOSE 8080
ENTRYPOINT ["/comp-lake"]
CMD ["serve", "--db", "/data/comp_lake.duckdb", "--port", "8080"]
```

Target image: ~25MB. Includes binary + seed data. DuckDB file is created on first run or mounted as a volume.

## MCP Server Tools

```
tool: get_compliance_score
  params: entity (string), framework (string, optional)
  returns: score, badge, controls_total, controls_passing, controls_stale

tool: get_coverage_gaps
  params: entity (string), framework (string, optional), limit (int)
  returns: prioritised list of untested/stale/failing controls

tool: recommend_experiments
  params: entity (string), framework (string, optional)
  returns: suggested experiments to close highest-priority gaps

tool: explain_control
  params: control_id (string)
  returns: control details, cross-framework mappings, evidence requirements

tool: get_cross_framework_map
  params: control_id (string)
  returns: all controls in other frameworks that map to this one

resource: framework_summaries
  returns: list of all frameworks with metadata and control counts
```

## Tumult Integration Path

Tumult currently owns `RegulatoryMapping`, `RegulatoryRequirement`, and `ResilienceScore` in `tumult-core/src/types.rs`. The migration:

1. **Phase 1**: comp-lake builds and serves framework data independently
2. **Phase 2**: tumult reads control IDs from comp-lake (DuckDB attach or Parquet)
3. **Phase 3**: tumult's `RegulatoryMapping.requirements[].id` references comp-lake control IDs
4. **Phase 4**: tumult writes evidence to comp-lake after experiment runs
5. **Phase 5**: tumult's `ResilienceScore` weights come from comp-lake config
6. **Phase 6**: tumult-core's compliance types become thin wrappers around comp-lake types

Same path for chaostooling, any other consumer.

## Decision Log

| Decision | Rationale |
|----------|-----------|
| Rust over Python | Container size (25MB vs 800MB), startup time, type safety for scoring models, DuckDB/Arrow/MCP crates proven in tumult |
| DORA as EU pivot (not 800-53) | European product, DORA is mandatory for target sector, direct mappings to ISO/NIS2/CRA are more natural than routing through US framework |
| NIST 800-53 as global secondary pivot | Best cross-reference ecosystem (OLIR), bridges to PCI DSS and SOC 2 |
| DuckDB + Parquet (not Iceberg/Flink) | Scale is thousands of controls, not millions of rows. No streaming needed. DuckDB analytical queries + Parquet versioning is sufficient. |
| Evidence freshness over time-decay function | Simpler model: evidence is fresh or stale. No gradual decay curve to calibrate. `expires_at > NOW()` is the entire freshness check. |
| Simple average for roll-up | Avoids premature complexity. Can add project-criticality weighting later. |
| Separate crates (core/harvesters/storage/api) | Clean dependency graph. Core has zero IO dependencies. Storage can be tested with in-memory DuckDB. API is thin layer over storage. |
