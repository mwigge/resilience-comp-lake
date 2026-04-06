# Compliance Scoring Methodology

**Version**: 1.0-draft
**Date**: 2026-04-06
**Status**: Draft — must be approved before implementation

---

## Purpose

Define a reproducible, evidence-based methodology for computing compliance scores that measure how well a team's resilience testing covers regulatory requirements. The score must be:

- **Deterministic**: same inputs produce same score
- **Auditable**: every score point traces to specific evidence and controls
- **Time-aware**: stale evidence reduces score without manual intervention
- **Multi-dimensional**: scores at every granularity (experiment → platform) and across frameworks
- **Cross-referenceable**: one experiment can satisfy controls in multiple frameworks

---

## 1. Framework Coverage Model

### 1.1 Control Universe

For each framework, only controls marked `testing_relevant = true` contribute to the score. This filters out controls that are purely procedural, governance, or documentation-focused.

**Testing-relevant criteria**: A control is testing-relevant if compliance can be demonstrated through:
- Running a chaos/resilience experiment
- Executing automated tests (unit, integration, E2E)
- Performing penetration testing or vulnerability scanning
- Measuring operational metrics (DORA deployment metrics, MTTR)
- Running a GameDay exercise

Controls that require only documentation, policy, or organisational measures (e.g., "appoint a CISO") are `testing_relevant = false`.

### 1.2 Initial Framework Scope

| Framework | Region | Total Controls (est.) | Testing-Relevant (est.) |
|-----------|--------|----------------------|------------------------|
| DORA (2022/2554) | EU | ~80 articles | ~25 |
| ISO 27001:2022 Annex A | Global | 93 controls | ~30 |
| PCI DSS 4.0.1 | Global | ~250 requirements | ~60 |
| NIST 800-53 rev5 | US/Global | 1189 controls | ~150 |
| NIST CSF 2.0 | US/Global | 106 subcategories | ~40 |
| CRA (2024/2847) | EU | ~45 articles | ~15 |
| NIS2 (2022/2555) | EU | ~45 articles | ~20 |
| GDPR (2016/679) | EU | 99 articles | ~5 (Art. 32, 35 only) |

---

## 2. Evidence Model

### 2.1 Evidence Types

| Type | Description | Example Source |
|------|-------------|---------------|
| `chaos_experiment` | Single resilience experiment result | tumult, chaostooling |
| `gameday` | Multi-experiment coordinated exercise | tumult GameDay |
| `pen_test` | Penetration test finding/report | manual, tooling |
| `vuln_scan` | Vulnerability scan result | NVD correlation, Snyk |
| `dora_metric` | DORA DevOps metric measurement | CI/CD pipeline |
| `scorecard` | OpenSSF Scorecard assessment | Scorecard API |
| `audit_finding` | External audit result | manual |
| `unit_test` | Unit/integration test coverage | pytest, vitest |
| `integration_test` | Integration test result | test suites |

### 2.2 Evidence Result Values

| Result | Score Contribution | Meaning |
|--------|-------------------|---------|
| `pass` | 1.0 | Control fully satisfied by this evidence |
| `partial` | 0.5 | Control partially addressed (e.g., test ran but recovery was incomplete) |
| `fail` | 0.0 | Control explicitly tested and not met |
| `not_tested` | 0.0 | No evidence exists for this control |

### 2.3 Evidence Freshness

Evidence expires after a type-specific period. Expired evidence contributes 0.0 to the score, identical to `not_tested`.

| Evidence Type | Freshness Period | Rationale |
|---------------|-----------------|-----------|
| `chaos_experiment` | 90 days | Systems change; quarterly re-validation |
| `gameday` | 180 days | Larger effort, semi-annual cadence |
| `pen_test` | 365 days | Annual cycle, aligned with PCI/DORA |
| `vuln_scan` | 30 days | New CVEs appear daily |
| `dora_metric` | 30 days | Operational metrics drift quickly |
| `scorecard` | 14 days | Changes with every merged PR |
| `audit_finding` | 365 days | Annual audit cycle |
| `unit_test` | 30 days | CI runs frequently; stale = suspicious |
| `integration_test` | 60 days | Less frequent than unit tests |

**Freshness check**: `evidence.expires_at > NOW()`. Binary fresh/stale — no gradual decay curve.

**Rationale for binary model**: A gradual decay function (e.g., exponential) requires calibration parameters that are hard to justify empirically. The binary model is simpler, auditable, and creates clear incentives: either your evidence is current or it isn't.

---

## 3. Score Computation

### 3.1 Entity-Framework Score

The base score for one entity (e.g., project "svc-auth") against one framework (e.g., DORA):

```
score(entity, framework) =
    |controls_with_fresh_passing_evidence|
    ÷ |controls_testing_relevant|
    × 100

Where:
  controls_testing_relevant = { c ∈ framework.controls | c.testing_relevant = true }
  controls_with_fresh_passing_evidence = {
      c ∈ controls_testing_relevant |
      ∃ e ∈ evidence :
          e.entity_id = entity
          ∧ e.control_id = c.control_id
          ∧ e.result = 'pass'
          ∧ e.expires_at > NOW()
  }
```

Range: 0.0 to 100.0

**Multiple evidence per control**: If multiple evidence records exist for the same (entity, control), use the **best fresh result**. A `pass` trumps a `partial` which trumps a `fail`. Only fresh evidence counts.

### 3.2 Cross-Framework Score Enhancement

When control mappings exist between frameworks, evidence can propagate:

```
cross_enhanced_score(entity, framework_A) =
    direct_controls_passing(entity, A)
    + Σ indirect_controls_passing(entity, B→A)
    ─────────────────────────────────────────
    controls_testing_relevant(A)
    × 100

Where:
  indirect_controls_passing(entity, B→A) = {
      c_a ∈ A.controls |
      ∃ mapping(c_b → c_a) with confidence ≥ medium
      ∧ c_b has fresh passing evidence for entity
      ∧ c_a does NOT have direct fresh evidence
  }
```

Rules:
- Mapped evidence only fills gaps — direct evidence always takes precedence
- Only `medium` and `high` confidence mappings propagate evidence
- `low` confidence mappings are informational only (shown in reports, not scored)
- A control filled by mapped evidence counts as 1.0 (not weighted by confidence)

**Rationale**: Weighting by confidence adds complexity without clear benefit. The confidence filter (≥ medium) already excludes weak mappings. Within the passing set, a control is either covered or not.

### 3.3 Hierarchical Roll-Up

```
team_score(team, framework)     = AVG(score(project, framework)) ∀ project ∈ team
unit_score(unit, framework)     = AVG(team_score(team, framework)) ∀ team ∈ unit
platform_score(plat, framework) = AVG(unit_score(unit, framework)) ∀ unit ∈ platform
```

Simple arithmetic mean. No weighting by project size or criticality in v1.

**Future**: Add optional `criticality` weight to `org_hierarchy` for weighted averages.

### 3.4 Aggregate Score (All Frameworks)

```
aggregate_score(entity) = AVG(score(entity, framework)) ∀ active frameworks
```

Simple average across all frameworks. This is the "headline" number for gamification.

---

## 4. Badge System

### 4.1 Per-Framework Badges

| Badge | Threshold | Visual |
|-------|-----------|--------|
| None | < 50% | - |
| Bronze | >= 50% | Colour: #CD7F32 |
| Silver | >= 70% | Colour: #C0C0C0 |
| Gold | >= 85% | Colour: #FFD700 |
| Platinum | >= 95% | Colour: #E5E4E2 |

### 4.2 Cross-Framework Badges

| Badge | Criteria |
|-------|----------|
| DORA Ready | Gold+ on DORA + Gold+ on ISO 27001 |
| PCI Champion | Gold+ on PCI DSS + Silver+ on NIST 800-53 |
| EU Compliant | Silver+ on DORA + Silver+ on NIS2 + Silver+ on CRA |
| Full Spectrum | Silver+ on ALL active frameworks |
| Resilience Leader | Platinum on any 3 frameworks |

### 4.3 Status Indicators

| Indicator | Condition |
|-----------|-----------|
| Stale Warning | > 25% of evidence is expired |
| Trending Up | Score increased >= 5 points vs 30 days ago |
| Trending Down | Score decreased >= 5 points vs 30 days ago |
| Stable | Score changed < 5 points vs 30 days ago |

### 4.4 Badge Transitions

Badge transitions (e.g., Silver → Gold) are events that consumers can subscribe to. Useful for:
- Team notifications ("Congratulations, team-alpha reached Gold on DORA!")
- Compliance officer dashboards
- LLM/AQE context ("team-alpha just achieved Gold, recommend next steps toward Platinum")

---

## 5. Harvest Cadence

| Source | Cadence | Trigger |
|--------|---------|---------|
| NVD CVEs | Daily | Scheduled (delta by `lastModStartDate`) |
| OpenSSF Scorecard | Weekly | Scheduled (per configured repo list) |
| EUR-Lex CELLAR | Monthly | Scheduled (SPARQL query by CELEX ID) |
| NIST OSCAL | On release | GitHub release webhook or monthly poll |
| PCI DSS | On version bump | Manual trigger + quarterly staleness check |
| ISO 27001 | On version bump | Manual trigger + quarterly staleness check |
| NIS2 | Monthly | Same harvester as DORA (EUR-Lex) |
| CRA | Monthly | Same harvester as DORA (EUR-Lex) |

### 5.1 Harvest Versioning

Each harvest produces a Parquet snapshot:
```
data/frameworks/dora/2026-04-06.parquet
data/frameworks/dora/2026-05-06.parquet
```

DuckDB always loads the latest snapshot. Previous snapshots are retained for audit trail and time-travel queries ("what was the control set when we last scored in January?").

---

## 6. Correlation Model

### 6.1 The Dual-Pivot Approach

```
EU Pivot (DORA):
  DORA ←→ ISO 27001    (direct, EU adoption)
  DORA ←→ NIS2          (direct, same EU legislative package)
  DORA ←→ CRA           (direct, DORA Ch.V ↔ CRA Art.10-13)
  DORA ←→ GDPR Art.32   (direct, security of processing)

Global Pivot (NIST 800-53):
  800-53 ←→ CSF 2.0     (official NIST mapping)
  800-53 ←→ PCI DSS 4.0 (NIST OLIR mapping)
  800-53 ←→ ISO 27001   (community OSCAL profile)

Bridge:
  ISO 27001 appears in BOTH pivots → bridges EU and global frameworks
```

### 6.2 Cross-Correlation Queries

The `v_cross_framework_map` view enables queries like:

- "Which DORA articles does our database failover experiment satisfy?"
- "If we pass PCI DSS 11.4, which NIST and DORA controls are also covered?"
- "What's the cheapest experiment to close gaps in both DORA and ISO 27001?"

---

## 7. Limitations and Future Work

### 7.1 Known Limitations (v1)

- **No severity weighting in score**: A HIGH control counts the same as a LOW control. Intentional simplification — severity is visible in gap analysis for prioritisation.
- **No partial credit for partial evidence**: `partial` result counts as 0.5, but there's no gradient. An experiment that "almost" recovered counts the same as one that barely started.
- **Simple average roll-up**: No project-criticality weighting. A low-risk internal tool counts the same as a payment processing service.
- **Binary freshness**: No gradual decay. Evidence is 100% fresh until expiry, then 0%.
- **Manual PCI DSS / ISO 27001 control import**: No API for these; initial data loaded from structured files.

### 7.2 Future Enhancements

- **Severity-weighted scoring**: `score = Σ(severity_weight × evidence_result) / Σ(severity_weight)` where HIGH=3, MODERATE=2, LOW=1
- **Project criticality weighting**: Add `criticality` field to `org_hierarchy`, use in roll-up
- **Gradual freshness decay**: Exponential decay curve starting at 80% of freshness period
- **Confidence-weighted cross-framework**: Mapped evidence weighted by mapping confidence rather than binary threshold
- **SOC 2 framework**: Add when US consumers emerge
- **ENISA guidance**: Incorporate ENISA resilience testing guidance as supplementary controls
- **Evidence quality scoring**: Not just pass/fail but quality of the test (blast radius, duration, realism)
