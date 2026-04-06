# Compliance Scoring Model

## Overview

The compliance score measures how well a team's resilience testing covers regulatory requirements. Scores are computed at every granularity level (experiment, project, team, business unit, platform) and across multiple compliance frameworks simultaneously.

## Score Formula

For a single entity against a single framework:

```
Score = (controls_passing / controls_total) * 100
```

Where:
- **controls_total** = number of testing-relevant controls in the framework
- **controls_passing** = number of controls with at least one fresh, passing evidence record

## Testing Relevance

Not all controls in a framework require testing. A control is `testing_relevant = true` if compliance can be demonstrated through:

- Running a chaos/resilience experiment
- Executing automated tests (unit, integration, E2E)
- Performing penetration testing or vulnerability scanning
- Measuring operational metrics (DORA deployment metrics, MTTR)
- Running a GameDay exercise

Controls requiring only documentation, policy, or organisational measures are excluded from scoring.

## Evidence Types and Freshness

Each evidence type has a validity period. Evidence expires automatically — scores degrade over time without manual intervention.

| Evidence Type | Validity | Example Source |
|--------------|----------|----------------|
| Chaos Experiment | 90 days | tumult, Gremlin, Litmus |
| GameDay | 180 days | tumult GameDay |
| Penetration Test | 365 days | manual, Cobalt, HackerOne |
| Vulnerability Scan | 30 days | Snyk, Trivy, NVD correlation |
| DORA Metric | 30 days | deployment frequency, MTTR |
| Scorecard | 14 days | OpenSSF Scorecard |
| Audit Finding | 365 days | SOC 2, ISO audit |
| Unit Test | 30 days | pytest, cargo test |
| Integration Test | 60 days | E2E suites |

### Freshness Rules

- Evidence is **fresh** if `now <= expires_at`
- Evidence is **stale** if `now > expires_at`
- Only fresh evidence counts toward the score
- Stale evidence is tracked separately (`controls_stale`) as a warning signal

## Multiple Evidence Per Control

When multiple evidence records exist for the same (entity, control) pair:

- Only fresh evidence is considered
- The **best result** wins: Pass > Partial > Fail
- `Pass` counts toward `controls_passing`
- `Partial` counts toward `controls_covered` but NOT `controls_passing`
- `Fail` counts toward `controls_covered` but NOT `controls_passing`

## Badge Tiers

Badges provide a human-readable summary of compliance posture:

| Badge | Score Threshold | Meaning |
|-------|----------------|---------|
| None | < 50% | Insufficient testing coverage |
| Bronze | >= 50% | Basic coverage — at least half of controls tested |
| Silver | >= 70% | Good coverage — most critical controls tested |
| Gold | >= 85% | Strong coverage — systematic testing programme |
| Platinum | >= 95% | Excellent coverage — near-complete testing |

## Cross-Framework Enhancement

The scoring engine can enhance scores using cross-framework control mappings:

1. For each testing-relevant control **without** direct fresh evidence:
2. Check if a **mapped** control in another framework has fresh passing evidence
3. Only mappings with confidence >= Medium are considered
4. If found, credit the control as passing

**Rules:**
- Direct evidence always takes precedence over mapped evidence
- Low confidence mappings never propagate evidence
- Score is capped at 100.0

### Example

```
DORA Art.25 (Resilience Testing) maps to ISO A.8.8 (Vulnerability Management)

Entity has:
- No direct evidence for DORA Art.25
- Passing evidence for ISO A.8.8 (from a vulnerability scan)
- Mapping confidence: High

Result: DORA Art.25 gets credit from ISO A.8.8 → DORA score improves
```

## Hierarchical Roll-Up

Scores aggregate upward through the org hierarchy using arithmetic mean:

```
Platform score = mean(Unit scores)
Unit score     = mean(Team scores)
Team score     = mean(Project scores)
```

## Trend Detection

Score trends are computed by comparing current score to the score 30 days ago:

| Trend | Condition |
|-------|-----------|
| Improving | delta >= +5 points |
| Stable | -5 < delta < +5 |
| Degrading | delta <= -5 points |

A **stale warning** fires when more than 25% of evidence backing a score has expired.

## Coverage Gaps

The system identifies untested/undertested controls and prioritises them:

1. **HIGH severity + never tested** (highest priority)
2. **HIGH severity + stale evidence**
3. **HIGH severity + failing**
4. **MODERATE severity + never tested**
5. ... and so on

This produces an actionable "test next" list for each entity.
