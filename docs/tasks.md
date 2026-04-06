# Tasks: resilience-comp-lake

**Estimation guide**: S = < half day, M = 1 day, L = 2-3 days, XL = 3-5 days

---

## Phase 0: Foundation

> Goal: compilable workspace with core types, scoring logic, DuckDB storage, and CI. No network calls, no API server. Everything testable locally.

### T0.1: Workspace scaffolding [S]

Create the Rust workspace at `api_projects/resilience_comp_lake/`.

**Deliverables**:
- `Cargo.toml` workspace root with members: `comp-lake-core`, `comp-lake-harvesters`, `comp-lake-storage`, `comp-lake-api`, `comp-lake-cli`
- Each crate has `src/lib.rs` (or `src/main.rs` for cli) with a placeholder
- Workspace-level dependencies matching tumult conventions: `serde`, `serde_json`, `tokio`, `thiserror`, `anyhow`, `chrono`, `uuid`
- `.gitignore`, `clippy.toml`, `rustfmt.toml`
- `cargo check --workspace` passes

**Acceptance**:
- [ ] `cargo check --workspace` compiles with zero warnings
- [ ] `cargo clippy --workspace -- -D warnings -W clippy::pedantic` clean
- [ ] Crate dependency graph: core → (nothing), storage → core, harvesters → core, api → core + storage, cli → all

### T0.2: Core models — frameworks and controls [M]

Implement the compliance framework domain types in `comp-lake-core/src/models/`.

**Deliverables**:
- `framework.rs`: `Framework` struct (framework_id, name, version, region, authority, is_pivot, effective_date, sunset_date, celex_id, eli_uri, harvest_source, last_harvested)
- `framework.rs`: `FrameworkId` newtype (String, validated)
- `framework.rs`: `Region` enum: `Eu`, `Global`, `Us`
- `framework.rs`: `HarvestSource` enum: `CellarSparql`, `OscalGithub`, `NvdApi`, `ScorecardApi`, `Manual`
- `control.rs`: `Control` struct (control_id, framework_id, article_ref, chapter_ref, title, description, family, severity, testing_relevant, parent_id)
- `control.rs`: `ControlId` newtype (String, validated)
- `control.rs`: `Severity` enum: `High`, `Moderate`, `Low`
- `control.rs`: `ControlFamily` — String newtype for grouping
- `mapping.rs`: `ControlMapping` struct (source_control, target_control, relationship, confidence, direction, provenance)
- `mapping.rs`: `MappingRelationship` enum: `Equivalent`, `Partial`, `Supplements`, `DerivedFrom`
- `mapping.rs`: `Confidence` enum: `Low`, `Medium`, `High` (reuse tumult's pattern)
- `mapping.rs`: `MappingDirection` enum: `Bidirectional`, `SourceToTarget`
- `mapping.rs`: `MappingProvenance` enum: `EbaMaping`, `NistOlir`, `OscalProfile`, `Manual`
- All types: `#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]`
- All types: `#[must_use]` on constructors, builder pattern where >4 fields

**Depends on**: T0.1
**Acceptance**:
- [ ] All types compile, serialize to JSON, deserialize from JSON
- [ ] Unit tests for serialization round-trip
- [ ] `Confidence` enum has `as_weight() -> f64` method: High=1.0, Medium=0.7, Low=0.4

### T0.3: Core models — evidence and org hierarchy [M]

**Deliverables**:
- `evidence.rs`: `Evidence` struct (evidence_id, entity_id, control_id, evidence_type, source_system, result, score, metadata, observed_at, expires_at)
- `evidence.rs`: `EvidenceId` newtype (UUID)
- `evidence.rs`: `EvidenceType` enum: `ChaosExperiment`, `GameDay`, `PenTest`, `VulnScan`, `DoraMetric`, `Scorecard`, `AuditFinding`, `UnitTest`, `IntegrationTest`
- `evidence.rs`: `EvidenceResult` enum: `Pass`, `Fail`, `Partial` with `as_score() -> f64` method (1.0, 0.0, 0.5)
- `evidence.rs`: `SourceSystem` — String newtype (e.g., "tumult", "chaostooling")
- `org.rs`: `OrgEntity` struct (entity_id, entity_type, name, parent_id)
- `org.rs`: `EntityId` newtype (String)
- `org.rs`: `EntityType` enum: `Platform`, `Unit`, `Team`, `Project` with ordering (Platform > Unit > Team > Project)

**Depends on**: T0.1
**Acceptance**:
- [ ] All types compile, serde round-trip
- [ ] `EntityType` implements `Ord` with Platform > Unit > Team > Project
- [ ] `EvidenceResult::as_score()` returns correct f64

### T0.4: Evidence freshness rules [S]

**Deliverables**:
- `freshness.rs`: `freshness_period(evidence_type: &EvidenceType) -> chrono::Duration`
  - ChaosExperiment: 90 days
  - GameDay: 180 days
  - PenTest: 365 days
  - VulnScan: 30 days
  - DoraMetric: 30 days
  - Scorecard: 14 days
  - AuditFinding: 365 days
  - UnitTest: 30 days
  - IntegrationTest: 60 days
- `freshness.rs`: `compute_expires_at(observed_at: DateTime<Utc>, evidence_type: &EvidenceType) -> DateTime<Utc>`
- `freshness.rs`: `is_fresh(evidence: &Evidence, now: DateTime<Utc>) -> bool`

**Depends on**: T0.3
**Acceptance**:
- [ ] Unit tests for each evidence type's freshness period
- [ ] `is_fresh` returns false when `now > expires_at`
- [ ] `compute_expires_at` correctly adds the type-specific duration

### T0.5: Scoring engine — entity-framework score [L]

The core scoring algorithm per the methodology doc.

**Deliverables**:
- `scoring/engine.rs`: `compute_entity_framework_score(entity_id, framework_id, controls, evidence, now) -> ComplianceScore`
- `scoring/mod.rs`: `ComplianceScore` struct: score (0.0-100.0), controls_total, controls_covered, controls_passing, controls_stale, badge
- `scoring/badges.rs`: `BadgeTier` enum: `None`, `Bronze`, `Silver`, `Gold`, `Platinum` with `from_score(f64) -> BadgeTier`
- `scoring/badges.rs`: `CrossFrameworkBadge` enum: `DoraReady`, `PciChampion`, `EuCompliant`, `FullSpectrum`, `ResilienceLeader`
- `scoring/badges.rs`: `evaluate_cross_badges(scores: &[ComplianceScore]) -> Vec<CrossFrameworkBadge>`
- Logic:
  - Only controls where `testing_relevant = true` count
  - Only evidence where `expires_at > now` counts
  - Multiple evidence per (entity, control): use best fresh result
  - Score = (controls_passing / controls_total) * 100
  - Badge = threshold lookup

**Depends on**: T0.2, T0.3, T0.4
**Acceptance**:
- [ ] Score of empty evidence set = 0.0
- [ ] Score of all controls passing with fresh evidence = 100.0
- [ ] Stale evidence (expires_at < now) does not count toward score
- [ ] `partial` result counts toward `controls_covered` but not `controls_passing` (score uses pass only)
- [ ] Multiple evidence per control: best fresh result wins
- [ ] Badge thresholds: None(<50), Bronze(>=50), Silver(>=70), Gold(>=85), Platinum(>=95)
- [ ] Cross-framework badges evaluate correctly from score sets

### T0.6: Scoring engine — cross-framework enhancement [M]

**Deliverables**:
- `scoring/cross_framework.rs`: `enhance_with_mappings(direct_score, entity_id, framework, mappings, all_evidence, now) -> ComplianceScore`
- Logic:
  - For each control in target framework without direct fresh evidence:
    - Check if a mapped control in another framework has fresh passing evidence
    - Only consider mappings with confidence >= Medium
    - If found, count the control as passing (1.0, not weighted)
  - Direct evidence always takes precedence over mapped evidence
  - Cap score at 100.0

**Depends on**: T0.5
**Acceptance**:
- [ ] Mapped evidence fills gaps: control without direct evidence gets credit from mapped control
- [ ] Direct evidence overrides: control with direct failing evidence stays failing even if mapped evidence passes
- [ ] Low confidence mappings do NOT propagate evidence
- [ ] Score is capped at 100.0
- [ ] Unit test with known mapping graph and evidence set, verify cross-enhanced score

### T0.7: Scoring engine — hierarchical roll-up [M]

**Deliverables**:
- `scoring/rollup.rs`: `rollup_scores(entity_scores: &[(EntityId, ComplianceScore)], hierarchy: &[OrgEntity]) -> Vec<(EntityId, ComplianceScore)>`
- `scoring/rollup.rs`: `aggregate_score(framework_scores: &[ComplianceScore]) -> ComplianceScore` — avg across all frameworks
- `scoring/trend.rs`: `Trend` enum: `Improving`, `Stable`, `Degrading` (reuse tumult's pattern)
- `scoring/trend.rs`: `compute_trend(current_score: f64, score_30d_ago: f64) -> Trend` — >=+5 improving, <=-5 degrading, else stable
- `scoring/trend.rs`: `StaleWarning` — true if >25% of evidence is expired

**Depends on**: T0.5, T0.3
**Acceptance**:
- [ ] Team score = arithmetic mean of project scores
- [ ] Unit score = arithmetic mean of team scores
- [ ] Platform score = arithmetic mean of unit scores
- [ ] Aggregate score = arithmetic mean of all framework scores
- [ ] Trend thresholds: >=+5 = Improving, <=-5 = Degrading, else Stable
- [ ] Stale warning when >25% evidence expired
- [ ] Empty children → parent score is 0.0 (not NaN)

### T0.8: DuckDB schema and migrations [L]

**Deliverables**:
- `comp-lake-storage/src/duckdb.rs`: `create_schema(conn)` — creates all tables if not exist
- `comp-lake-storage/src/duckdb.rs`: `migrate(conn)` — schema versioning via `schema_meta` table (tumult-analytics pattern)
- Tables: `frameworks`, `controls`, `control_mappings`, `org_hierarchy`, `evidence`, `harvest_log`, `schema_meta`
- All DDL exactly as specified in design.md
- Insert/upsert functions for each table
- `comp-lake-storage/src/duckdb.rs`: `CompLakeStore` struct wrapping `duckdb::Connection`

**Depends on**: T0.2, T0.3
**Acceptance**:
- [ ] Schema creates cleanly on empty DuckDB (in-memory and file-based)
- [ ] Schema migration detects version and upgrades
- [ ] All foreign key relationships enforced
- [ ] Insert + read round-trip for each table
- [ ] Upsert (insert or replace) for frameworks, controls, evidence

### T0.9: Arrow schema definitions [S]

**Deliverables**:
- `comp-lake-storage/src/arrow.rs`: `frameworks_schema() -> Schema`
- `comp-lake-storage/src/arrow.rs`: `controls_schema() -> Schema`
- `comp-lake-storage/src/arrow.rs`: `evidence_schema() -> Schema`
- `comp-lake-storage/src/arrow.rs`: `scores_schema() -> Schema`
- Conversion functions: `frameworks_to_record_batch`, `controls_to_record_batch`, etc.
- Mirror tumult-analytics `arrow_convert.rs` patterns

**Depends on**: T0.2, T0.3
**Acceptance**:
- [ ] Each schema matches the corresponding DuckDB table columns
- [ ] Conversion functions produce valid RecordBatches
- [ ] Round-trip: struct → RecordBatch → struct

### T0.10: DuckDB analytical views [L]

The core contract — these views ARE the API for embedded consumers.

**Deliverables**:
- `comp-lake-storage/src/views.rs`: `create_views(conn)` — creates all views
- Views exactly as specified in design.md:
  - `v_scores` — per-entity, per-framework score with badge
  - `v_rollup_scores` — recursive CTE for team/unit/platform roll-up
  - `v_coverage_gaps` — prioritised list of untested/stale/failing controls
  - `v_cross_framework_map` — control-to-control mappings across frameworks
  - `v_evidence_freshness` — freshness dashboard per entity per evidence type
- `comp-lake-storage/src/views.rs`: `refresh_views(conn)` — drops and recreates (for schema changes)

**Depends on**: T0.8
**Acceptance**:
- [ ] All views create successfully on a populated DuckDB
- [ ] `v_scores` produces correct scores matching the scoring engine output (cross-validation)
- [ ] `v_rollup_scores` recursive CTE terminates and matches `rollup_scores()` output
- [ ] `v_coverage_gaps` correctly prioritises HIGH+never_tested over MODERATE+stale
- [ ] `v_cross_framework_map` returns bidirectional mappings
- [ ] `v_evidence_freshness` counts fresh vs stale correctly

### T0.11: Parquet export/import [M]

**Deliverables**:
- `comp-lake-storage/src/export.rs`: `export_parquet(batch: &RecordBatch, path: &Path)` — Zstd-compressed
- `comp-lake-storage/src/export.rs`: `import_parquet(path: &Path) -> Vec<RecordBatch>`
- `comp-lake-storage/src/export.rs`: `export_framework_snapshot(conn, framework_id, output_dir)` — controls + mappings as Parquet
- `comp-lake-storage/src/export.rs`: `export_scores_snapshot(conn, output_dir)` — all scores as Parquet
- `comp-lake-storage/src/export.rs`: `export_duckdb_file(conn, output_path)` — full database copy for distribution
- Arrow IPC export (optional, for tumult FFI consumption)

**Depends on**: T0.9, T0.10
**Acceptance**:
- [ ] Parquet files are Zstd-compressed
- [ ] Round-trip: export → import → compare = identical
- [ ] Framework snapshot contains all controls and their mappings
- [ ] Exported DuckDB file is self-contained (tables + views, queryable standalone)

### T0.12: Dockerfile [S]

**Deliverables**:
- `docker/Dockerfile`: multi-stage build
  - Stage 1: `rust:1.89-slim` builder, `--target x86_64-unknown-linux-musl`, `--release`
  - Stage 2: `gcr.io/distroless/static-debian12:nonroot`
  - Copy binary + `data/seed/`
  - Expose 8080, entrypoint `comp-lake serve`
- `docker/.dockerignore`
- `Makefile` target: `docker-build`, `docker-run`

**Depends on**: T0.1
**Acceptance**:
- [ ] `docker build` succeeds
- [ ] Image size < 50MB (target ~25MB)
- [ ] Container starts and responds to health check in < 1s
- [ ] Runs as non-root user

### T0.13: Pre-commit hooks and CI config [S]

**Deliverables**:
- Pre-commit hook: `cargo fmt --check && cargo clippy -- -D warnings -W clippy::pedantic && cargo test --workspace && cargo audit`
- `.github/workflows/ci.yml` (or equivalent for local use)
- `Makefile` with targets: `check`, `fmt`, `lint`, `test`, `audit`, `build`, `docker-build`

**Depends on**: T0.1
**Acceptance**:
- [ ] Pre-commit hook blocks commits with warnings or test failures
- [ ] `make check` runs full quality gate
- [ ] 300000ms timeout for git commit (test suite may be slow with DuckDB)

---

## Phase 1: Harvesters

> Goal: all framework data can be fetched from authoritative sources and normalised into the DuckDB schema. Each harvester is independently testable with mocked HTTP responses.

### T1.1: Harvester trait and common types [S]

**Deliverables**:
- `comp-lake-harvesters/src/harvester.rs`: `Harvester` async trait
  - `fn name(&self) -> &str`
  - `fn frameworks(&self) -> &[FrameworkId]`
  - `async fn harvest(&self, config: &HarvestConfig) -> Result<HarvestResult>`
  - `fn cadence(&self) -> HarvestCadence`
- `HarvestCadence` enum: `Daily`, `Weekly`, `Monthly`, `OnRelease`, `OnVersion`
- `HarvestConfig` struct: http client, timeout, API keys, output dir
- `HarvestResult` struct: framework, controls, mappings, snapshot_version, harvested_at
- `HarvestLog` struct: harvest_id, framework_id, started_at, completed_at, status, controls_added, controls_updated, error_message

**Depends on**: T0.2
**Acceptance**:
- [ ] Trait is object-safe (`dyn Harvester`)
- [ ] `HarvestResult` can be written directly to DuckDB via storage layer

### T1.2: EUR-Lex CELLAR harvester [L]

Fetches EU legislation via SPARQL endpoint at `publications.europa.eu/webapi/rdf/sparql`.

**Deliverables**:
- `comp-lake-harvesters/src/cellar.rs`: `CellarHarvester` implementing `Harvester`
- SPARQL queries for:
  - DORA (CELEX: 32022R2554) — articles, chapters, recitals
  - CRA (CELEX: 32024R2847) — articles
  - NIS2 (CELEX: 32022L2555) — articles
  - GDPR (CELEX: 32016R0679) — Art. 32, 35 only
- Response parsing: SPARQL JSON result format → `Control` structs
- `testing_relevant` classification for each article (based on methodology doc criteria)
- Delta detection: compare harvested controls with existing, flag additions/changes

**Depends on**: T1.1, T0.8
**Acceptance**:
- [ ] Harvests DORA and produces >=20 testing-relevant controls
- [ ] Each control has: article_ref, chapter_ref, title, description, severity
- [ ] Integration test with mocked SPARQL response
- [ ] Writes harvest_log entry on completion
- [ ] Idempotent: running twice produces same result

### T1.3: NIST OSCAL harvester [L]

Fetches NIST 800-53 and CSF from GitHub OSCAL content repo.

**Deliverables**:
- `comp-lake-harvesters/src/oscal.rs`: `OscalHarvester` implementing `Harvester`
- JSON parsing for:
  - SP 800-53 rev5 catalog (`NIST_SP-800-53_rev5_catalog.json`)
  - CSF 2.0 profile
- Extracts: control families, controls, enhancements, assessment methods
- Maps OSCAL `group` → `family`, `control.id` → `control_id`, `control.title` → `title`
- Extracts `testing_relevant` from assessment methods (EXAMINE/INTERVIEW/TEST — only TEST-tagged controls are testing-relevant)
- Extracts official 800-53 ↔ CSF mappings from OSCAL profile

**Depends on**: T1.1, T0.8
**Acceptance**:
- [ ] Harvests 800-53 and produces 1000+ controls (full catalog)
- [ ] `testing_relevant` filter reduces to ~150 controls
- [ ] Each control has: family, severity (from OSCAL baseline: HIGH/MODERATE/LOW)
- [ ] 800-53 ↔ CSF mappings extracted as `ControlMapping` records
- [ ] Integration test with fixture JSON (subset of real OSCAL catalog)

### T1.4: NIST NVD harvester [M]

Fetches CVE data from `services.nvd.nist.gov/rest/json/cves/2.0`.

**Deliverables**:
- `comp-lake-harvesters/src/nvd.rs`: `NvdHarvester` implementing `Harvester`
- Pagination: 2000 results per page, follow `startIndex`
- Delta mode: use `lastModStartDate` / `lastModEndDate` for incremental updates
- API key support: via env var `NVD_API_KEY` (50 req/30s with key vs 5/30s without)
- Rate limiting: respect API limits with backoff
- Extracts: CVE-ID, CVSS v3.1 base score, CWE-IDs, affected CPE configurations, published date
- Stores as evidence-adjacent data (CVEs map to vuln_scan evidence type)

**Depends on**: T1.1, T0.8
**Acceptance**:
- [ ] Can fetch first page (2000 CVEs) successfully
- [ ] Delta mode: only fetches CVEs modified since last harvest
- [ ] Rate limiting prevents 429 errors
- [ ] Integration test with mocked NVD response
- [ ] API key loaded from env var, graceful fallback to lower rate

### T1.5: OpenSSF Scorecard harvester [M]

Fetches security scores from `api.securityscorecards.dev`.

**Deliverables**:
- `comp-lake-harvesters/src/scorecard.rs`: `ScorecardHarvester` implementing `Harvester`
- Configuration: list of GitHub repos to score (from config file)
- For each repo: `GET /projects/github.com/{owner}/{repo}`
- Extracts: 18 check scores (0-10), overall score, reasons, check documentation URLs
- Maps scorecard checks to evidence records (evidence_type = `Scorecard`)
- Maps scorecard checks to relevant controls (e.g., "Code-Review" → NIST SA-11, DORA Art.25)

**Depends on**: T1.1, T0.8
**Acceptance**:
- [ ] Fetches scorecard for a configured list of repos
- [ ] Each check becomes an evidence record with score normalised to 0.0-1.0
- [ ] Check-to-control mapping defined for at least 10 of 18 checks
- [ ] Integration test with mocked scorecard response
- [ ] Handles repos without scorecards gracefully (skip, log)

### T1.6: Manual/seed data loader [M]

Loads frameworks that don't have APIs: PCI DSS 4.0, ISO 27001:2022.

**Deliverables**:
- `comp-lake-harvesters/src/manual.rs`: `ManualLoader` implementing `Harvester`
- Seed data format: TOML or JSON files in `data/seed/`
  - `data/seed/pci_dss_4.toml` — PCI DSS 4.0.1 testing-relevant requirements
  - `data/seed/iso_27001_2022.toml` — ISO 27001:2022 Annex A testing-relevant controls
- Loader reads seed files, produces `HarvestResult`
- Seed files include: control_id, title, description, family, severity, testing_relevant

**Depends on**: T1.1, T0.8
**Acceptance**:
- [ ] Seed files parseable and valid
- [ ] Loader produces correct `HarvestResult` matching seed data
- [ ] PCI DSS seed: >=40 testing-relevant requirements
- [ ] ISO 27001 seed: >=25 testing-relevant controls
- [ ] Idempotent: loading twice doesn't duplicate

### T1.7: Harvest scheduler [M]

Runs harvesters on their configured cadences.

**Deliverables**:
- `comp-lake-harvesters/src/scheduler.rs`: `HarvestScheduler`
- Reads cadence from each harvester's `fn cadence()`
- Tracks last-harvest timestamps per framework (from `harvest_log` table)
- On tick: checks which harvesters are due, runs them sequentially
- CLI integration: `comp-lake harvest --all` (run all due) or `comp-lake harvest --framework dora` (run specific)
- On-demand trigger: force-run regardless of cadence

**Depends on**: T1.1 through T1.6, T0.8
**Acceptance**:
- [ ] Scheduler correctly identifies which harvesters are due based on last_harvested + cadence
- [ ] `--all` runs all due harvesters
- [ ] `--framework <id>` runs specific harvester regardless of cadence
- [ ] Harvest log entries created for each run

### T1.8: Harvest versioning and snapshots [M]

**Deliverables**:
- After each successful harvest: export Parquet snapshot to `data/frameworks/{framework_id}/{date}.parquet`
- DuckDB always loads latest version (based on `last_harvested`)
- `comp-lake export snapshot --framework dora` CLI command
- Snapshot includes: all controls for that framework + their mappings at that point in time
- Retention: keep last 12 snapshots per framework (configurable)

**Depends on**: T0.11, T1.7
**Acceptance**:
- [ ] Harvest produces Parquet snapshot file
- [ ] Filename format: `{framework_id}/{YYYY-MM-DD}.parquet`
- [ ] DuckDB queries use latest snapshot data
- [ ] Retention policy deletes old snapshots beyond configured limit
- [ ] Time-travel query works: "load DORA controls as of 2026-01-15"

---

## Phase 2: Cross-Framework Mappings

> Goal: all frameworks cross-referenced via the dual-pivot model (DORA for EU, 800-53 for global). ISO 27001 bridges both pivots.

### T2.1: DORA ↔ ISO 27001 mappings [L]

**Deliverables**:
- Seed file: `data/seed/mappings/dora_iso27001.toml`
- Map DORA testing-relevant articles to ISO 27001:2022 Annex A controls
- Key mappings:
  - DORA Art.5-6 (ICT risk management) ↔ ISO A.5.x, A.8.x
  - DORA Art.17-19 (incident reporting) ↔ ISO A.5.24-A.5.28
  - DORA Art.24-27 (resilience testing) ↔ ISO A.8.8, A.5.35-A.5.37
  - DORA Art.28-30 (third-party risk) ↔ ISO A.5.19-A.5.23
- Each mapping: relationship, confidence, direction, provenance ("eba_mapping" or "manual")

**Depends on**: T0.8, T5.1, T5.2
**Acceptance**:
- [ ] >=15 testing-relevant mappings with confidence >= Medium
- [ ] Mappings load into `control_mappings` table
- [ ] `v_cross_framework_map` returns correct results for DORA ↔ ISO queries

### T2.2: DORA ↔ NIS2 mappings [M]

**Deliverables**:
- Seed file: `data/seed/mappings/dora_nis2.toml`
- Key mappings:
  - DORA Art.5-6 ↔ NIS2 Art.21 (risk management measures)
  - DORA Art.17-19 ↔ NIS2 Art.23 (incident reporting)
  - DORA Art.24-27 ↔ NIS2 Art.21(2)(e) (testing and auditing)
- High confidence — same EU legislative package, intentional alignment

**Depends on**: T0.8, T5.1, T5.5
**Acceptance**:
- [ ] >=10 testing-relevant mappings
- [ ] Most mappings are High confidence (same EU package)

### T2.3: DORA ↔ CRA mappings [M]

**Deliverables**:
- Seed file: `data/seed/mappings/dora_cra.toml`
- Key mappings:
  - DORA Art.28-30 (third-party ICT) ↔ CRA Art.10-13 (obligations of manufacturers)
  - DORA Art.5-6 (risk management) ↔ CRA Annex I (essential cybersecurity requirements)

**Depends on**: T0.8, T5.1, T5.6
**Acceptance**:
- [ ] >=8 testing-relevant mappings
- [ ] Confidence levels documented with rationale

### T2.4: DORA ↔ GDPR Art.32/35 mappings [S]

**Deliverables**:
- Seed file: `data/seed/mappings/dora_gdpr.toml`
- DORA Art.5 (ICT risk) ↔ GDPR Art.32 (security of processing)
- DORA Art.24 (general requirements for testing) ↔ GDPR Art.35 (DPIA)
- Small mapping set — GDPR scope is narrow for testing

**Depends on**: T0.8, T5.1, T5.7
**Acceptance**:
- [ ] 3-5 mappings, well-documented
- [ ] Confidence levels reflect the indirect relationship

### T2.5: NIST 800-53 ↔ CSF 2.0 mappings [M]

**Deliverables**:
- Auto-extracted from OSCAL harvester (T1.3) — official NIST mapping
- Validate extracted mappings against known CSF ↔ 800-53 alignment
- All mappings are High confidence (official NIST source)

**Depends on**: T1.3
**Acceptance**:
- [ ] >=80 mappings extracted from OSCAL
- [ ] All marked High confidence, provenance = "oscal_profile"
- [ ] Spot-check: CSF PR.IP-10 maps to 800-53 CP-4 (contingency testing)

### T2.6: NIST 800-53 ↔ PCI DSS 4.0 mappings [L]

**Deliverables**:
- Source: NIST OLIR (Online Informative References) cross-reference data
- Fetch or load OLIR mapping data for PCI DSS 4.0 → 800-53
- Parse and load into `control_mappings`
- Medium confidence (community-contributed OLIR)

**Depends on**: T0.8, T1.3, T1.6
**Acceptance**:
- [ ] >=30 testing-relevant mappings
- [ ] Provenance = "nist_olir"
- [ ] Spot-check: PCI 11.4 (pen testing) maps to 800-53 CA-8

### T2.7: ISO 27001 bridge (EU ↔ global pivot) [M]

**Deliverables**:
- ISO 27001 ↔ NIST 800-53 mappings (from community OSCAL profile or manual)
- This creates the bridge: DORA ↔ ISO 27001 ↔ NIST 800-53 ↔ PCI DSS
- Validate that transitive paths exist (DORA → ISO → 800-53 → PCI)

**Depends on**: T2.1, T2.6
**Acceptance**:
- [ ] >=20 ISO 27001 ↔ 800-53 mappings
- [ ] Transitive path query: "DORA Art.25 → ? → ? → PCI 11.4" returns a valid chain
- [ ] `v_cross_framework_map` shows the full chain

### T2.8: Mapping validation and coverage report [M]

**Deliverables**:
- CLI command: `comp-lake validate mappings`
- Report:
  - Total mappings per framework pair
  - Confidence distribution (High/Medium/Low) per pair
  - Orphan controls: testing-relevant controls with zero mappings
  - Coverage heatmap: framework × framework matrix showing mapping density
- Warnings for: low-coverage pairs, high proportion of Low confidence mappings

**Depends on**: T2.1 through T2.7
**Acceptance**:
- [ ] Report generates successfully
- [ ] Identifies any testing-relevant controls with zero cross-framework mappings
- [ ] Coverage matrix shows all framework pairs

---

## Phase 3: API Surface

> Goal: all comp-lake data accessible via REST API, MCP server, and CLI. Each endpoint has OpenAPI docs.

### T3.1: axum REST API scaffold [M]

**Deliverables**:
- `comp-lake-api/src/rest/app.rs`: axum `Router` with:
  - Shared state: `CompLakeStore` (DuckDB connection)
  - CORS middleware
  - Request logging (tracing)
  - Health check: `GET /healthz`
  - API versioning: all routes under `/api/v1/`
- Error handling: structured JSON error responses
- OpenAPI generation (utoipa or manual spec)

**Depends on**: T0.8
**Acceptance**:
- [ ] Server starts and responds to `/healthz`
- [ ] JSON error responses for 404, 400, 500
- [ ] Request tracing with trace_id propagation

### T3.2: GET /api/v1/frameworks [S]

**Deliverables**:
- List all frameworks with metadata
- Response: `{ frameworks: [{ framework_id, name, version, region, authority, effective_date, last_harvested, control_count_total, control_count_testing_relevant }] }`

**Depends on**: T3.1
**Acceptance**:
- [ ] Returns all frameworks
- [ ] control_count fields are accurate

### T3.3: GET /api/v1/controls [M]

**Deliverables**:
- Query params: `framework`, `testing_relevant`, `severity`, `family`, `search` (text search in title/description)
- Pagination: `offset`, `limit` (default 50, max 500)
- Response: `{ controls: [...], total, offset, limit }`
- Each control includes its cross-framework mappings inline

**Depends on**: T3.1
**Acceptance**:
- [ ] Filter by framework returns only that framework's controls
- [ ] `testing_relevant=true` filter works
- [ ] Pagination returns correct total count
- [ ] Cross-framework mappings included per control

### T3.4: GET /api/v1/scores/{entity_id} [M]

**Deliverables**:
- Returns compliance scores for an entity
- Query params: `framework` (optional — all frameworks if omitted)
- Response includes: score, badge, controls_total, controls_passing, controls_covered, controls_stale, trend, stale_warning
- If entity is team/unit/platform: includes roll-up data and children scores
- Historical: `?history=true` returns last 12 monthly scores for trend charting

**Depends on**: T3.1, T0.10
**Acceptance**:
- [ ] Score matches `v_scores` view output
- [ ] Roll-up entities show children
- [ ] History endpoint returns monthly snapshots
- [ ] Non-existent entity returns 404

### T3.5: GET /api/v1/gaps/{entity_id} [M]

**Deliverables**:
- Returns prioritised coverage gaps
- Query params: `framework` (optional), `limit` (default 20)
- Response: prioritised list from `v_coverage_gaps` with: control_id, title, severity, framework, gap_reason, priority_rank
- Each gap includes: suggested evidence types to close it

**Depends on**: T3.1, T0.10
**Acceptance**:
- [ ] Gaps prioritised: HIGH+never_tested first
- [ ] Suggested evidence types are sensible for the control
- [ ] Respects framework filter

### T3.6: POST /api/v1/evidence [M]

**Deliverables**:
- Ingest evidence from any source system
- Request body:
  ```json
  {
    "entity_id": "svc-auth",
    "control_id": "DORA-25.1",
    "evidence_type": "chaos_experiment",
    "source_system": "tumult",
    "result": "pass",
    "score": 0.92,
    "metadata": { "experiment_id": "...", "journal_path": "..." }
  }
  ```
- `evidence_id` auto-generated (UUID)
- `observed_at` defaults to now
- `expires_at` computed from evidence_type freshness rules
- Validates: entity_id exists, control_id exists, result is valid enum value
- Batch ingest: `POST /api/v1/evidence/batch` with array body

**Depends on**: T3.1, T0.4, T0.8
**Acceptance**:
- [ ] Single evidence ingest returns created evidence with ID
- [ ] Batch ingest accepts array, returns array of results
- [ ] Invalid entity_id or control_id returns 400 with clear error
- [ ] expires_at correctly computed from evidence_type
- [ ] Scores recomputable after evidence ingest

### T3.7: POST /api/v1/harvest/trigger [S]

**Deliverables**:
- Trigger on-demand harvest
- Request body: `{ "framework": "dora" }` or `{ "all": true }`
- Returns harvest job ID, status can be polled
- Requires auth token (bearer) — configurable via env var

**Depends on**: T3.1, T1.7
**Acceptance**:
- [ ] Triggers harvest for specified framework
- [ ] Returns immediately with job ID (harvest runs async)
- [ ] Auth required — 401 without token

### T3.8: GET /api/v1/mappings/{control_id} [S]

**Deliverables**:
- Returns all cross-framework mappings for a control
- Includes both directions (source and target)
- Response: mapped controls with relationship, confidence, provenance

**Depends on**: T3.1
**Acceptance**:
- [ ] Returns all mappings regardless of direction
- [ ] Includes framework metadata for each mapped control

### T3.9: MCP server — core tools [L]

**Deliverables**:
- `comp-lake-api/src/mcp/server.rs`: MCP server using `rust-mcp-sdk`
- Tools:
  - `get_compliance_score(entity, framework?)` → score, badge, trend, gaps summary
  - `get_coverage_gaps(entity, framework?, limit?)` → prioritised gap list
  - `recommend_experiments(entity, framework?)` → suggested experiments to close gaps, with reasoning
  - `explain_control(control_id)` → control details, text, cross-framework mappings, what evidence is needed
- Resource: `framework_summaries` → list of all frameworks with metadata and control counts
- Bearer token auth (same as REST)

**Depends on**: T3.1, T0.10
**Acceptance**:
- [ ] All 4 tools callable via MCP protocol
- [ ] `recommend_experiments` produces actionable suggestions (not just "test this control")
- [ ] `explain_control` includes the regulation text and cross-framework context
- [ ] Framework summaries resource is readable

### T3.10: CLI subcommands [M]

**Deliverables**:
- `comp-lake-cli/src/main.rs` with clap subcommands:
  - `serve` — start REST + MCP servers (configurable ports)
  - `harvest` — run harvesters (`--all`, `--framework <id>`)
  - `score` — compute and display scores (`--entity <id>`, `--framework <id>`)
  - `export` — export Parquet/DuckDB (`--format parquet|duckdb`, `--output <path>`)
  - `validate` — validate mappings, seed data, schema
  - `init` — create empty DuckDB with schema + seed data
- Configuration via TOML file (`comp-lake.toml`) and env vars

**Depends on**: all prior tasks
**Acceptance**:
- [ ] `comp-lake init` creates a working database from seed data
- [ ] `comp-lake serve` starts API server
- [ ] `comp-lake harvest --framework dora` runs harvester
- [ ] `comp-lake score --entity svc-auth` prints score table
- [ ] `comp-lake export --format duckdb --output ./dist/comp_lake.duckdb` produces distributable file

---

## Phase 4: Testing & Quality

> Goal: >=95% test coverage on core and storage. All scoring edge cases validated. DuckDB views cross-checked against Rust scoring engine.

### T4.1: Scoring engine unit tests [L]

**Deliverables**:
- Test module in `comp-lake-core/src/scoring/`
- Cases:
  - Empty evidence set → score 0.0, badge None
  - All controls passing with fresh evidence → score 100.0, badge Platinum
  - Mixed results (pass/fail/partial) → correct proportional score
  - All evidence stale → score 0.0 (evidence exists but expired)
  - Single control with multiple evidence records → best fresh result wins
  - `partial` result: counts as covered but not passing
  - Cross-framework: mapped evidence fills gaps
  - Cross-framework: direct evidence overrides mapped
  - Cross-framework: Low confidence mappings do NOT propagate
  - Roll-up: team = avg(projects), handles empty children
  - Aggregate: avg across all frameworks
  - Trend: +5 = improving, -5 = degrading, between = stable
  - Badge thresholds: boundary values (49.9 = None, 50.0 = Bronze, etc.)
  - Cross-framework badges: verify all 5 badge types

**Depends on**: T0.5, T0.6, T0.7
**Acceptance**:
- [ ] >=30 test cases
- [ ] 100% branch coverage on scoring engine
- [ ] All boundary values tested

### T4.2: Freshness rules unit tests [S]

**Deliverables**:
- Test each evidence type's freshness period
- Test `is_fresh` at boundary (exactly at expiry = stale, 1 second before = fresh)
- Test `compute_expires_at` for each type

**Depends on**: T0.4
**Acceptance**:
- [ ] All 9 evidence types tested
- [ ] Boundary conditions covered

### T4.3: DuckDB view cross-validation tests [L]

**Deliverables**:
- Integration tests in `comp-lake-storage/tests/`
- For a known dataset (fixed controls, evidence, org hierarchy):
  - Compute scores via Rust scoring engine
  - Query `v_scores` via DuckDB SQL
  - Assert they produce identical results
- Same for: `v_rollup_scores`, `v_coverage_gaps`

**Depends on**: T0.5, T0.10
**Acceptance**:
- [ ] Rust engine and DuckDB views produce identical scores for the test dataset
- [ ] Roll-up view matches Rust roll-up function
- [ ] Gap priorities match in both implementations

### T4.4: Harvester integration tests [L]

**Deliverables**:
- Mock HTTP server (wiremock-rs) for each harvester
- Test fixtures: realistic API responses (subset of real data)
- For each harvester:
  - Feed mock response → assert correct Controls produced
  - Verify testing_relevant classification
  - Verify idempotency (harvest twice = same result)

**Depends on**: T1.2 through T1.6
**Acceptance**:
- [ ] Each harvester has >=5 integration tests
- [ ] Fixtures are realistic (based on actual API responses captured during exploration)
- [ ] No network calls in test suite

### T4.5: End-to-end test [XL]

**Deliverables**:
- Full pipeline test:
  1. `comp-lake init` (empty DB + seed data)
  2. Run manual loader (PCI DSS, ISO 27001)
  3. Run OSCAL harvester (mocked)
  4. Load cross-framework mappings
  5. Create sample org hierarchy
  6. Ingest sample evidence via REST API
  7. Query scores via REST API
  8. Verify scores, badges, gaps
  9. Export DuckDB file
  10. Verify exported file is self-contained and queryable

**Depends on**: all Phase 0-3 tasks
**Acceptance**:
- [ ] Full pipeline runs in CI without external network
- [ ] Final scores match expected values for the test dataset
- [ ] Exported DuckDB file usable by an external DuckDB client

### T4.6: Badge transition tests [M]

**Deliverables**:
- Test scenarios:
  - None → Bronze (cross threshold with new evidence)
  - Silver → Gold (cross threshold)
  - Gold → Silver (evidence expires, score drops)
  - Cross-framework badge earned
  - Cross-framework badge lost
  - Stale warning triggered / cleared

**Depends on**: T0.5
**Acceptance**:
- [ ] All transition directions tested (up and down)
- [ ] Stale warning threshold (25%) verified

### T4.7: API contract tests [M]

**Deliverables**:
- REST API tests using axum test client
- For each endpoint: happy path, validation errors, 404s, auth failures
- MCP tool tests: verify tool call → response format
- Response schema validation against OpenAPI spec (if generated)

**Depends on**: T3.1 through T3.10
**Acceptance**:
- [ ] Every REST endpoint has >=3 test cases
- [ ] Error responses have correct HTTP status codes and JSON structure
- [ ] MCP tools return valid tool results

---

## Phase 5: Seed Data & Validation

> Goal: all 8 frameworks loaded with testing-relevant controls, cross-referenced, and validated against the methodology doc. A reference DuckDB file is exported for consumer testing.

### T5.1: DORA testing-relevant controls [L]

**Deliverables**:
- Seed file: `data/seed/frameworks/dora.toml`
- Chapter II (Art. 5-16): ICT risk management — ~8 testing-relevant controls
- Chapter III (Art. 17-23): Incident reporting — ~5 testing-relevant controls
- Chapter IV (Art. 24-27): Resilience testing — ~8 testing-relevant controls (primary)
- Chapter V (Art. 28-44): Third-party risk — ~4 testing-relevant controls
- Each control: control_id, article_ref, chapter_ref, title, description (from EUR-Lex), family, severity, testing_relevant
- Severity assignment rationale documented in comments

**Depends on**: T1.2 or manual research
**Acceptance**:
- [ ] >=20 testing-relevant DORA controls
- [ ] All Chapter IV articles represented
- [ ] Severity assignments justified

### T5.2: ISO 27001:2022 testing-relevant controls [M]

**Deliverables**:
- Seed file: `data/seed/frameworks/iso_27001_2022.toml`
- Annex A controls relevant to resilience testing:
  - A.5.x (Organisational) — incident management, testing, supplier relations
  - A.8.x (Technological) — vulnerability management, configuration management, redundancy
- Each control: control_id (e.g., "ISO-A.8.8"), title, description, family, severity

**Depends on**: T1.6
**Acceptance**:
- [ ] >=25 testing-relevant ISO 27001 controls
- [ ] Covers both organisational and technological categories

### T5.3: PCI DSS 4.0.1 testing-relevant requirements [M]

**Deliverables**:
- Seed file: `data/seed/frameworks/pci_dss_4.toml`
- Key requirements:
  - Req 5 (malware protection)
  - Req 6 (secure development)
  - Req 10 (logging and monitoring)
  - Req 11 (security testing — pen tests, vuln scans, IDS/IPS)
  - Req 12.10 (incident response)

**Depends on**: T1.6
**Acceptance**:
- [ ] >=40 testing-relevant PCI DSS requirements
- [ ] Requirement 11 (security testing) fully represented

### T5.4: NIST 800-53 + CSF testing-relevant controls [M]

**Deliverables**:
- Via OSCAL harvester (T1.3) or seed file
- Key families: CA (Assessment), CP (Contingency Planning), IR (Incident Response), RA (Risk Assessment), SI (System and Information Integrity)
- CSF subcategories: PR.IP, DE.CM, RS.RP, RC.RP

**Depends on**: T1.3
**Acceptance**:
- [ ] >=100 testing-relevant 800-53 controls
- [ ] >=30 testing-relevant CSF subcategories
- [ ] Correct severity from OSCAL baseline (HIGH/MODERATE/LOW)

### T5.5: NIS2 testing-relevant articles [M]

**Deliverables**:
- Seed file: `data/seed/frameworks/nis2.toml`
- Art. 21 (cybersecurity risk-management measures) — sub-paragraphs
- Art. 23 (reporting obligations)
- Art. 24 (use of European cybersecurity certification schemes)

**Depends on**: T1.2 or manual research
**Acceptance**:
- [ ] >=15 testing-relevant NIS2 controls
- [ ] Clearly distinguished from DORA (despite overlap)

### T5.6: CRA testing-relevant articles [M]

**Deliverables**:
- Seed file: `data/seed/frameworks/cra.toml`
- Art. 10-13 (obligations of manufacturers)
- Annex I (essential cybersecurity requirements)
- Focus on: vulnerability handling, security testing, update mechanisms

**Depends on**: T1.2 or manual research
**Acceptance**:
- [ ] >=12 testing-relevant CRA controls
- [ ] Annex I requirements included

### T5.7: GDPR Art. 32, 35 [S]

**Deliverables**:
- Seed file: `data/seed/frameworks/gdpr_testing.toml`
- Art. 32(1)(d): process for regularly testing, assessing and evaluating effectiveness of technical and organisational measures
- Art. 35: Data Protection Impact Assessment (testing aspects only)

**Depends on**: T1.2 or manual research
**Acceptance**:
- [ ] 3-5 testing-relevant GDPR controls
- [ ] Clearly scoped to testing only

### T5.8: Sample org hierarchy [S]

**Deliverables**:
- Seed file: `data/seed/sample_org.toml`
- Structure:
  ```
  payments-platform (platform)
  ├── core (unit)
  │   ├── team-alpha (team)
  │   │   ├── svc-auth (project)
  │   │   ├── svc-payments (project)
  │   │   └── svc-ledger (project)
  │   └── team-bravo (team)
  │       ├── svc-notifications (project)
  │       └── svc-audit-log (project)
  └── gateway (unit)
      └── team-gateway (team)
          ├── api-gateway (project)
          └── rate-limiter (project)
  ```

**Depends on**: T0.8
**Acceptance**:
- [ ] Hierarchy loads correctly
- [ ] Parent-child relationships intact
- [ ] Usable for roll-up testing

### T5.9: Sample evidence set + scoring validation [L]

**Deliverables**:
- Seed file: `data/seed/sample_evidence.toml`
- Evidence for `svc-auth` project:
  - 3 chaos experiments (2 pass, 1 fail) covering DORA Art.25 controls
  - 1 GameDay (pass) covering multiple DORA + ISO controls
  - 2 vuln scans (1 pass, 1 partial)
  - 1 stale experiment (observed_at 4 months ago, expired)
- Expected scores pre-computed by hand:
  - svc-auth DORA: X% (documented calculation)
  - svc-auth ISO: Y%
  - team-alpha DORA: Z% (avg of projects)
- Manual calculation documented in `docs/validation-worksheet.md`

**Depends on**: T0.5, T0.10, T5.1 through T5.8
**Acceptance**:
- [ ] Computed scores match hand-calculated expected values
- [ ] Stale evidence correctly excluded
- [ ] Cross-framework evidence propagation verified
- [ ] Roll-up verified at team/unit/platform levels
- [ ] Badges assigned correctly

### T5.10: Reference DuckDB file export [S]

**Deliverables**:
- `comp-lake export --format duckdb --output dist/comp_lake_reference.duckdb`
- Contains: all seed frameworks, controls, mappings, sample org, sample evidence, pre-computed views
- Documented: what's in the file, how to attach from tumult/DuckDB CLI
- `README.md` for the export: connection example in Rust, Python, SQL

**Depends on**: T5.9
**Acceptance**:
- [ ] File is self-contained (<10MB)
- [ ] `duckdb dist/comp_lake_reference.duckdb "SELECT * FROM v_scores LIMIT 5"` works
- [ ] Views return data
- [ ] Documented connection examples work

---

## Dependency Graph (phases)

```
Phase 0 ──────┐
(Foundation)   │
               ├──▶ Phase 1 ──────┐
               │    (Harvesters)   │
               │                   ├──▶ Phase 2 ────┐
               │                   │    (Mappings)   │
               ├──▶ Phase 5.1-5.7 ─┘                │
               │    (Seed data)                      ├──▶ Phase 3
               │                                     │    (API)
               └──▶ Phase 4.1-4.2 ──────────────────┘       │
                    (Unit tests)                              │
                                                              ▼
                                               Phase 4.3-4.7 + Phase 5.8-5.10
                                               (Integration + Validation)
```

Critical path: T0.1 → T0.2/T0.3 → T0.5 → T0.8 → T0.10 → T1.x → T2.x → T3.x → T5.9
