# Cross-Framework Mappings

## The Dual-Pivot Model

Regulatory frameworks don't exist in isolation. A single resilience test can satisfy requirements in multiple frameworks simultaneously. The compliance data lake uses a **dual-pivot** architecture to enable this:

```
                    DORA (EU Pivot)
                   /    |    \    \
                NIS2   CRA  GDPR  ISO 27001 (Bridge)
                                     |
                              NIST 800-53 (Global Pivot)
                               /         \
                           CSF 2.0    PCI DSS 4.0
```

- **DORA** is the EU pivot — all EU frameworks map to DORA
- **NIST 800-53** is the global pivot — all global/US frameworks map to 800-53
- **ISO 27001** bridges both pivots — it maps to DORA via EBA guidelines AND to 800-53 via community profiles

This creates **transitive mapping paths**: `DORA → ISO 27001 → NIST 800-53 → PCI DSS`.

## Mapping Properties

Each mapping has:

| Property | Values | Meaning |
|----------|--------|---------|
| Relationship | Equivalent, Partial, Supplements, DerivedFrom | How closely the controls align |
| Confidence | High, Medium, Low | How confident we are in the mapping |
| Direction | Bidirectional, SourceToTarget | Whether the mapping works both ways |
| Provenance | EbaMapping, NistOlir, OscalProfile, Manual | Where the mapping came from |

### Confidence Levels

- **High** (weight 1.0): Official mapping from authoritative source (EBA, NIST OLIR, OSCAL profile) or same legislative package (DORA ↔ NIS2)
- **Medium** (weight 0.7): Community-contributed or expert-reviewed mapping
- **Low** (weight 0.4): Automated/inferred mapping — not used for cross-framework scoring

**Only High and Medium confidence mappings propagate evidence across frameworks.**

## Current Mappings

### DORA ↔ ISO 27001:2022 (20 mappings)

The strongest mapping pair. Based on EBA guidelines for DORA implementation.

| DORA | ISO 27001 | Relationship | Confidence |
|------|-----------|-------------|------------|
| Art. 5 (ICT risk management) | A.5.2 (Security roles) | Partial | High |
| Art. 9 (Protection) | A.8.7 (Malware), A.8.20 (Networks) | Partial | High/Medium |
| Art. 10 (Detection) | A.8.15 (Logging), A.8.16 (Monitoring) | Equivalent | High |
| Art. 11 (Response/recovery) | A.5.26 (Incident response) | Equivalent | High |
| Art. 17 (Incident process) | A.5.24 (Incident planning) | Equivalent | High |
| Art. 19 (Classification) | A.5.25 (Assessment) | Equivalent | High |
| Art. 24 (Testing general) | A.8.29 (Security testing) | Equivalent | High |
| Art. 25 (Testing tools) | A.8.8 (Vuln mgmt), A.8.25 (SDLC), A.8.29 | Equivalent/Partial | High/Medium |
| Art. 26 (TLPT) | A.5.35 (Independent review) | Partial | Medium |
| Art. 28 (Third-party) | A.5.19 (Supplier security), A.5.21 (ICT supply chain) | Equivalent/Partial | High |
| Art. 30 (Contracts) | A.5.20 (Supplier agreements) | Equivalent | High |

### DORA ↔ NIS2 (12 mappings)

High confidence — both directives are part of the same EU legislative package and intentionally aligned.

| DORA | NIS2 | Notes |
|------|------|-------|
| Art. 5-6 (Risk management) | Art. 21 (Risk measures) | Direct overlap |
| Art. 9-11 (Protection/detection/response) | Art. 21 (Risk measures) | NIS2 Art.21 is broad |
| Art. 17-20 (Incident management) | Art. 23 (Incident reporting) | Both require reporting to authorities |
| Art. 24-25 (Resilience testing) | Art. 21 (Testing and auditing) | NIS2 21(2)(e) specifically |
| Art. 28 (Third-party risk) | Art. 21 (Supply chain security) | NIS2 21(2)(d) specifically |

### DORA ↔ CRA (8 mappings)

Moderate confidence — CRA focuses on product security while DORA focuses on operational resilience, but they intersect on ICT risk and third-party obligations.

| DORA | CRA | Relationship |
|------|-----|-------------|
| Art. 5-6 (Risk framework) | Art. 10 (Manufacturer obligations) | Partial |
| Art. 9 (Protection) | Art. 11 (Vulnerability handling) | Partial |
| Art. 10 (Detection) | Art. 12 (Reporting obligations) | Partial |
| Art. 17 (Incidents) | Art. 14 (Incident reporting) | Partial |
| Art. 25 (Testing) | Art. 11 (Testing requirements) | Partial |
| Art. 28, 30 (Third-party) | Art. 13 (Importer/distributor obligations) | Equivalent/Partial |

### DORA ↔ GDPR Art. 32/35 (4 mappings)

Narrow scope — only GDPR's testing-relevant articles.

| DORA | GDPR | Confidence |
|------|------|-----------|
| Art. 5 (Risk framework) | Art. 32 (Security of processing) | Medium |
| Art. 9 (Protection) | Art. 32 (Security of processing) | Medium |
| Art. 24 (Testing general) | Art. 35 (DPIA) | Low |
| Art. 25 (Testing tools) | Art. 32 (Security of processing) | Medium |

## How Cross-Framework Scoring Works

### Scenario: One chaos experiment satisfies three frameworks

```
Team Alpha runs a database failover chaos experiment using tumult.

The experiment generates evidence:
  entity: proj-payments
  control: DORA-ART-25 (Testing of ICT tools)
  result: Pass

Cross-framework credit via mappings:
  DORA-ART-25 → ISO-A.8.8  (Equivalent, High confidence)  → ISO score improves
  DORA-ART-25 → ISO-A.8.29 (Partial, High confidence)     → ISO score improves
  DORA-ART-25 → NIS2-ART-21 (Partial, High confidence)    → NIS2 score improves

One experiment, three frameworks scored.
```

### Scenario: Mapped evidence fills a gap

```
Team Beta has no direct DORA testing evidence, but has:
  - ISO A.8.8 (Vulnerability management): Pass (from Snyk scan)
  - ISO A.5.24 (Incident planning): Pass (from GameDay)

Via DORA ↔ ISO mappings:
  ISO-A.8.8  → DORA-ART-25 (Equivalent, High)  → DORA gets credit
  ISO-A.5.24 → DORA-ART-17 (Equivalent, High)  → DORA gets credit

Result: Team Beta's DORA score improves from 0% to ~10%
  without running any DORA-specific tests.
```

## Mapping Seed Format

Mappings are stored as TOML seed files in `data/seed/mappings/`:

```toml
[[mappings]]
source = "DORA-ART-25"
target = "ISO-A.8.8"
relationship = "Equivalent"
confidence = "High"
direction = "Bidirectional"
provenance = "EbaMapping"
```

Load all mappings:
```bash
comp-lake --db demo.duckdb seed --data-dir data/seed
```

Query the cross-framework map via DuckDB view:
```sql
SELECT * FROM v_cross_framework_map
WHERE framework_a = 'DORA' AND framework_b = 'ISO-27001-2022';
```
