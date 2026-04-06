# Supported Compliance Frameworks

## Overview

The compliance data lake normalises regulatory frameworks into a unified control taxonomy. Each framework is harvested from its authoritative source, decomposed into individual controls, and tagged for testing relevance.

## Framework Registry

### EU Frameworks (DORA as pivot)

#### DORA — Digital Operational Resilience Act (2022/2554)

The primary EU pivot framework. DORA establishes ICT risk management, incident reporting, resilience testing, and third-party risk requirements for EU financial entities.

| Property | Value |
|----------|-------|
| Region | EU |
| Authority | European Parliament |
| Harvest source | EUR-Lex CELLAR (SPARQL) |
| Total articles | ~80 |
| Testing-relevant | ~20 |
| Key families | Risk Management, Incident Management, Resilience Testing, Third-Party Risk |

**Key testing-relevant articles:**
- Art. 5-6: ICT risk management framework
- Art. 9-13: Protection, detection, response, recovery, learning
- Art. 17-21: Incident management and reporting
- Art. 24-27: Resilience testing (general, TLPT, testers)
- Art. 28-30: Third-party risk management

#### NIS2 — Network and Information Security Directive (2022/2555)

EU cybersecurity directive complementing DORA. High overlap — both part of the same EU legislative package.

| Property | Value |
|----------|-------|
| Region | EU |
| Authority | European Parliament |
| Harvest source | EUR-Lex CELLAR (SPARQL) |
| Testing-relevant | ~20 |

**Key articles:** Art. 21 (risk management measures), Art. 23-25 (incident reporting)

#### CRA — Cyber Resilience Act (2024/2847)

EU regulation on cybersecurity requirements for products with digital elements.

| Property | Value |
|----------|-------|
| Region | EU |
| Authority | European Parliament |
| Harvest source | EUR-Lex CELLAR (SPARQL) |
| Testing-relevant | ~15 |

**Key articles:** Art. 10-13 (manufacturer obligations), Art. 14-18 (reporting)

#### GDPR — General Data Protection Regulation (2016/679)

Only testing-relevant articles included (narrow scope):
- Art. 32: Security of processing
- Art. 35: Data protection impact assessment

### Global Frameworks (NIST 800-53 as pivot)

#### NIST SP 800-53 Rev. 5

The global pivot framework. Comprehensive security and privacy controls for US federal systems, widely adopted internationally.

| Property | Value |
|----------|-------|
| Region | US / Global |
| Authority | NIST |
| Harvest source | OSCAL (GitHub JSON) |
| Total controls | ~1189 |
| Testing-relevant | ~150 (TEST assessment method) |
| Key families | AC, AU, CA, CM, CP, IA, IR, RA, SA, SC, SI, SR |

**Testing relevance is determined by OSCAL assessment methods:** only controls tagged with the TEST method (not just EXAMINE or INTERVIEW) are scoring-relevant.

#### NIST CSF 2.0

Cybersecurity Framework — higher-level categories mapped to 800-53 controls via official NIST OSCAL profiles.

| Property | Value |
|----------|-------|
| Region | US / Global |
| Authority | NIST |
| Harvest source | OSCAL (GitHub JSON) |
| Testing-relevant | ~40 subcategories |

#### ISO/IEC 27001:2022

The bridge framework connecting EU and global pivots. ISO 27001 maps bidirectionally to both DORA (via EBA guidelines) and NIST 800-53 (via community OSCAL profiles).

| Property | Value |
|----------|-------|
| Region | Global |
| Authority | ISO/IEC |
| Harvest source | Manual seed (TOML) |
| Annex A controls | 93 |
| Testing-relevant | ~30 |
| Key families | Organisational Controls, Technology Controls |

#### PCI DSS 4.0.1

Payment card industry standard. Mapped to NIST 800-53 via NIST OLIR cross-references.

| Property | Value |
|----------|-------|
| Region | Global |
| Authority | PCI SSC |
| Harvest source | Manual seed (TOML) |
| Testing-relevant | ~60 requirements |

## Harvest Sources

| Source | URL | Frameworks | Method |
|--------|-----|------------|--------|
| EUR-Lex CELLAR | `publications.europa.eu/webapi/rdf/sparql` | DORA, NIS2, CRA, GDPR | SPARQL queries by CELEX ID |
| NIST OSCAL | `github.com/usnistgov/oscal-content` | 800-53, CSF 2.0 | JSON catalog parsing |
| NIST NVD | `services.nvd.nist.gov/rest/json/cves/2.0` | CVE data | REST API with pagination |
| OpenSSF Scorecard | `api.securityscorecards.dev` | Supply chain security | REST API per repository |
| Manual/Seed | `data/seed/frameworks/*.toml` | ISO 27001, PCI DSS | TOML file loading |

## Harvest Cadence

| Cadence | Interval | Frameworks |
|---------|----------|------------|
| Daily | 24 hours | NVD (CVE data) |
| Weekly | 7 days | OpenSSF Scorecard |
| Monthly | 30 days | EUR-Lex CELLAR (EU legislation) |
| On Release | On new version | NIST OSCAL catalogs |
| On Version | Manual trigger | ISO 27001, PCI DSS seed data |
