use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::ComplianceScore;
use crate::models::control::{Control, ControlId};
use crate::models::evidence::{Evidence, EvidenceResult};
use crate::models::framework::FrameworkId;
use crate::models::org::EntityId;

/// Compute the compliance score for a single entity against a single framework.
///
/// Logic:
/// - Only controls where `testing_relevant = true` count
/// - Only evidence where `expires_at >= now` counts (fresh)
/// - Multiple evidence per (entity, control): best fresh result wins
/// - Score = (`controls_passing` / `controls_total`) * 100
/// - `Partial` counts toward `controls_covered` but NOT `controls_passing`
#[must_use]
pub fn compute_entity_framework_score(
    entity_id: &EntityId,
    framework_id: &FrameworkId,
    controls: &[Control],
    evidence: &[Evidence],
    now: DateTime<Utc>,
) -> ComplianceScore {
    let relevant_controls: Vec<&Control> = controls
        .iter()
        .filter(|c| c.framework_id == *framework_id && c.testing_relevant)
        .collect();

    let controls_total = relevant_controls.len();

    if controls_total == 0 {
        return ComplianceScore::new(framework_id.clone(), 0, 0, 0, 0);
    }

    // Build map: control_id -> best fresh evidence result for this entity
    let mut best_per_control: HashMap<&ControlId, BestEvidence> = HashMap::new();

    for ev in evidence {
        if ev.entity_id != *entity_id {
            continue;
        }

        let fresh = ev.expires_at > now;
        let entry = best_per_control
            .entry(&ev.control_id)
            .or_insert(BestEvidence {
                best_fresh_result: None,
                has_stale: false,
            });

        if fresh {
            let current_best = entry
                .best_fresh_result
                .map_or(-1.0, EvidenceResult::as_score);
            if ev.result.as_score() > current_best {
                entry.best_fresh_result = Some(ev.result);
            }
        } else {
            entry.has_stale = true;
        }
    }

    let mut controls_covered = 0;
    let mut controls_passing = 0;
    let mut controls_stale = 0;

    for ctrl in &relevant_controls {
        if let Some(best) = best_per_control.get(&ctrl.control_id) {
            if let Some(result) = best.best_fresh_result {
                controls_covered += 1;
                if result == EvidenceResult::Pass {
                    controls_passing += 1;
                }
            } else if best.has_stale {
                controls_stale += 1;
            }
        }
    }

    ComplianceScore::new(
        framework_id.clone(),
        controls_total,
        controls_covered,
        controls_passing,
        controls_stale,
    )
}

struct BestEvidence {
    best_fresh_result: Option<EvidenceResult>,
    has_stale: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::control::{ControlId, Severity};
    use crate::models::evidence::{EvidenceId, EvidenceType, SourceSystem};
    use crate::models::framework::FrameworkId;
    use crate::models::freshness::compute_expires_at;
    use crate::scoring::badges::BadgeTier;
    use chrono::TimeZone;

    /// Tolerance for f64 score comparisons.
    const SCORE_TOLERANCE: f64 = 1e-10;

    fn assert_score_eq(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < SCORE_TOLERANCE,
            "score mismatch: expected {expected}, got {actual}"
        );
    }

    /// Fixed reference time for deterministic tests.
    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 12, 12, 0, 0)
            .single()
            .expect("valid fixed timestamp")
    }

    fn fw_id() -> FrameworkId {
        FrameworkId::new("DORA").expect("valid framework ID")
    }

    fn entity_id() -> EntityId {
        EntityId::new("team-alpha")
    }

    fn make_control(id: &str, testing_relevant: bool) -> Control {
        Control::builder(
            ControlId::new(id).expect("valid control ID"),
            fw_id(),
            format!("Control {id}"),
        )
        .testing_relevant(testing_relevant)
        .severity(Severity::High)
        .build()
    }

    fn make_evidence(
        control_id: &str,
        result: EvidenceResult,
        observed_at: DateTime<Utc>,
        evidence_type: EvidenceType,
    ) -> Evidence {
        Evidence {
            evidence_id: EvidenceId::new(),
            entity_id: entity_id(),
            control_id: ControlId::new(control_id).expect("valid control ID"),
            evidence_type,
            source_system: SourceSystem::new("tumult"),
            result,
            score: None,
            metadata: serde_json::Value::Null,
            observed_at,
            expires_at: compute_expires_at(observed_at, &evidence_type),
        }
    }

    #[test]
    fn empty_evidence_scores_zero() {
        let controls = vec![make_control("C1", true), make_control("C2", true)];
        let now = fixed_now();
        let score = compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &[], now);
        assert_score_eq(score.score, 0.0);
        assert_eq!(score.controls_total, 2);
        assert_eq!(score.controls_covered, 0);
        assert_eq!(score.controls_passing, 0);
        assert_eq!(score.badge, BadgeTier::None);
    }

    #[test]
    fn all_passing_scores_100() {
        let controls = vec![make_control("C1", true), make_control("C2", true)];
        let now = fixed_now();
        let evidence = vec![
            make_evidence(
                "C1",
                EvidenceResult::Pass,
                now,
                EvidenceType::ChaosExperiment,
            ),
            make_evidence(
                "C2",
                EvidenceResult::Pass,
                now,
                EvidenceType::ChaosExperiment,
            ),
        ];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 100.0);
        assert_eq!(score.controls_passing, 2);
        assert_eq!(score.badge, BadgeTier::Platinum);
    }

    #[test]
    fn stale_evidence_does_not_count() {
        let controls = vec![make_control("C1", true)];
        let now = fixed_now();
        let old = now - chrono::Duration::days(200);
        let evidence = vec![make_evidence(
            "C1",
            EvidenceResult::Pass,
            old,
            EvidenceType::ChaosExperiment, // 90-day freshness
        )];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 0.0);
        assert_eq!(score.controls_stale, 1);
        assert_eq!(score.controls_covered, 0);
    }

    #[test]
    fn partial_counts_as_covered_not_passing() {
        let controls = vec![make_control("C1", true)];
        let now = fixed_now();
        let evidence = vec![make_evidence(
            "C1",
            EvidenceResult::Partial,
            now,
            EvidenceType::ChaosExperiment,
        )];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 0.0);
        assert_eq!(score.controls_covered, 1);
        assert_eq!(score.controls_passing, 0);
    }

    #[test]
    fn best_fresh_result_wins() {
        let controls = vec![make_control("C1", true)];
        let now = fixed_now();
        let evidence = vec![
            make_evidence(
                "C1",
                EvidenceResult::Fail,
                now,
                EvidenceType::ChaosExperiment,
            ),
            make_evidence("C1", EvidenceResult::Pass, now, EvidenceType::GameDay),
        ];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 100.0);
        assert_eq!(score.controls_passing, 1);
    }

    #[test]
    fn non_testing_relevant_controls_excluded() {
        let controls = vec![make_control("C1", true), make_control("C2", false)];
        let now = fixed_now();
        let evidence = vec![make_evidence(
            "C1",
            EvidenceResult::Pass,
            now,
            EvidenceType::ChaosExperiment,
        )];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_eq!(score.controls_total, 1);
        assert_score_eq(score.score, 100.0);
    }

    #[test]
    fn mixed_results_correct_score() {
        let controls = vec![
            make_control("C1", true),
            make_control("C2", true),
            make_control("C3", true),
            make_control("C4", true),
        ];
        let now = fixed_now();
        let evidence = vec![
            make_evidence(
                "C1",
                EvidenceResult::Pass,
                now,
                EvidenceType::ChaosExperiment,
            ),
            make_evidence(
                "C2",
                EvidenceResult::Pass,
                now,
                EvidenceType::ChaosExperiment,
            ),
            make_evidence(
                "C3",
                EvidenceResult::Fail,
                now,
                EvidenceType::ChaosExperiment,
            ),
            // C4: no evidence
        ];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 50.0);
        assert_eq!(score.controls_total, 4);
        assert_eq!(score.controls_covered, 3);
        assert_eq!(score.controls_passing, 2);
        assert_eq!(score.badge, BadgeTier::Bronze);
    }

    #[test]
    fn no_controls_scores_zero() {
        let now = fixed_now();
        let score = compute_entity_framework_score(&entity_id(), &fw_id(), &[], &[], now);
        assert_score_eq(score.score, 0.0);
        assert_eq!(score.controls_total, 0);
    }

    #[test]
    fn score_capped_at_100() {
        // Even if controls_passing somehow exceeds controls_total via ComplianceScore::new,
        // the score is capped. Here we verify engine itself never exceeds 100.
        let controls = vec![make_control("C1", true)];
        let now = fixed_now();
        let evidence = vec![
            make_evidence(
                "C1",
                EvidenceResult::Pass,
                now,
                EvidenceType::ChaosExperiment,
            ),
            make_evidence("C1", EvidenceResult::Pass, now, EvidenceType::GameDay),
        ];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert!(score.score <= 100.0);
        assert_score_eq(score.score, 100.0);
    }

    #[test]
    fn badge_bronze_boundary_at_50() {
        // 2 out of 4 passing = 50.0 => Bronze
        let controls = vec![
            make_control("C1", true),
            make_control("C2", true),
            make_control("C3", true),
            make_control("C4", true),
        ];
        let now = fixed_now();
        let evidence = vec![
            make_evidence(
                "C1",
                EvidenceResult::Pass,
                now,
                EvidenceType::ChaosExperiment,
            ),
            make_evidence(
                "C2",
                EvidenceResult::Pass,
                now,
                EvidenceType::ChaosExperiment,
            ),
        ];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 50.0);
        assert_eq!(score.badge, BadgeTier::Bronze);
    }

    #[test]
    fn badge_silver_boundary_at_70() {
        // 7 out of 10 passing = 70.0 => Silver
        let controls: Vec<Control> = (0..10)
            .map(|i| make_control(&format!("C{i}"), true))
            .collect();
        let now = fixed_now();
        let evidence: Vec<Evidence> = (0..7)
            .map(|i| {
                make_evidence(
                    &format!("C{i}"),
                    EvidenceResult::Pass,
                    now,
                    EvidenceType::ChaosExperiment,
                )
            })
            .collect();
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 70.0);
        assert_eq!(score.badge, BadgeTier::Silver);
    }

    #[test]
    fn badge_gold_boundary_at_85() {
        // 17 out of 20 passing = 85.0 => Gold
        let controls: Vec<Control> = (0..20)
            .map(|i| make_control(&format!("C{i}"), true))
            .collect();
        let now = fixed_now();
        let evidence: Vec<Evidence> = (0..17)
            .map(|i| {
                make_evidence(
                    &format!("C{i}"),
                    EvidenceResult::Pass,
                    now,
                    EvidenceType::ChaosExperiment,
                )
            })
            .collect();
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 85.0);
        assert_eq!(score.badge, BadgeTier::Gold);
    }

    #[test]
    fn badge_platinum_boundary_at_95() {
        // 19 out of 20 passing = 95.0 => Platinum
        let controls: Vec<Control> = (0..20)
            .map(|i| make_control(&format!("C{i}"), true))
            .collect();
        let now = fixed_now();
        let evidence: Vec<Evidence> = (0..19)
            .map(|i| {
                make_evidence(
                    &format!("C{i}"),
                    EvidenceResult::Pass,
                    now,
                    EvidenceType::ChaosExperiment,
                )
            })
            .collect();
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 95.0);
        assert_eq!(score.badge, BadgeTier::Platinum);
    }

    #[test]
    fn badge_none_below_50() {
        // 9 out of 20 passing = 45.0 => None
        let controls: Vec<Control> = (0..20)
            .map(|i| make_control(&format!("C{i}"), true))
            .collect();
        let now = fixed_now();
        let evidence: Vec<Evidence> = (0..9)
            .map(|i| {
                make_evidence(
                    &format!("C{i}"),
                    EvidenceResult::Pass,
                    now,
                    EvidenceType::ChaosExperiment,
                )
            })
            .collect();
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 45.0);
        assert_eq!(score.badge, BadgeTier::None);
    }

    #[test]
    fn deterministic_fixed_timestamps() {
        let now = Utc
            .with_ymd_and_hms(2026, 4, 12, 0, 0, 0)
            .single()
            .expect("valid fixed timestamp");
        let controls = vec![make_control("C1", true)];
        let evidence = vec![make_evidence(
            "C1",
            EvidenceResult::Pass,
            now,
            EvidenceType::ChaosExperiment,
        )];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 100.0);
        assert_eq!(score.badge, BadgeTier::Platinum);
    }

    #[test]
    fn evidence_for_different_entity_ignored() {
        let controls = vec![make_control("C1", true)];
        let now = fixed_now();
        let other_entity = EntityId::new("team-beta");
        let et = EvidenceType::ChaosExperiment;
        let evidence = vec![Evidence {
            evidence_id: EvidenceId::new(),
            entity_id: other_entity,
            control_id: ControlId::new("C1").expect("valid control ID"),
            evidence_type: et,
            source_system: SourceSystem::new("tumult"),
            result: EvidenceResult::Pass,
            score: None,
            metadata: serde_json::Value::Null,
            observed_at: now,
            expires_at: compute_expires_at(now, &et),
        }];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 0.0);
        assert_eq!(score.controls_covered, 0);
    }

    #[test]
    fn multi_evidence_stale_plus_fresh_uses_fresh() {
        let controls = vec![make_control("C1", true)];
        let now = fixed_now();
        let old = now - chrono::Duration::days(200);
        let evidence = vec![
            // Stale: observed 200 days ago, ChaosExperiment has 90-day window
            make_evidence(
                "C1",
                EvidenceResult::Pass,
                old,
                EvidenceType::ChaosExperiment,
            ),
            // Fresh: observed now
            make_evidence(
                "C1",
                EvidenceResult::Fail,
                now,
                EvidenceType::ChaosExperiment,
            ),
        ];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        // Fresh Fail wins over stale Pass
        assert_eq!(score.controls_covered, 1);
        assert_eq!(score.controls_passing, 0);
        assert_eq!(score.controls_stale, 0);
    }

    #[test]
    fn single_control_single_evidence_scores_100() {
        let controls = vec![make_control("C1", true)];
        let now = fixed_now();
        let evidence = vec![make_evidence(
            "C1",
            EvidenceResult::Pass,
            now,
            EvidenceType::ChaosExperiment,
        )];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_score_eq(score.score, 100.0);
        assert_eq!(score.controls_total, 1);
        assert_eq!(score.controls_covered, 1);
        assert_eq!(score.controls_passing, 1);
    }
}
