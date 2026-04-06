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
/// - Only evidence where `expires_at > now` counts (fresh)
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

    fn fw_id() -> FrameworkId {
        FrameworkId::new("DORA").unwrap()
    }

    fn entity_id() -> EntityId {
        EntityId::new("team-alpha")
    }

    fn make_control(id: &str, testing_relevant: bool) -> Control {
        Control::builder(
            ControlId::new(id).unwrap(),
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
            control_id: ControlId::new(control_id).unwrap(),
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
        let now = Utc::now();
        let score = compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &[], now);
        assert!((score.score - 0.0).abs() < f64::EPSILON);
        assert_eq!(score.controls_total, 2);
        assert_eq!(score.controls_covered, 0);
        assert_eq!(score.controls_passing, 0);
        assert_eq!(score.badge, BadgeTier::None);
    }

    #[test]
    fn all_passing_scores_100() {
        let controls = vec![make_control("C1", true), make_control("C2", true)];
        let now = Utc::now();
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
        assert!((score.score - 100.0).abs() < f64::EPSILON);
        assert_eq!(score.controls_passing, 2);
        assert_eq!(score.badge, BadgeTier::Platinum);
    }

    #[test]
    fn stale_evidence_does_not_count() {
        let controls = vec![make_control("C1", true)];
        let now = Utc::now();
        let old = now - chrono::Duration::days(200);
        let evidence = vec![make_evidence(
            "C1",
            EvidenceResult::Pass,
            old,
            EvidenceType::ChaosExperiment, // 90-day freshness
        )];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert!((score.score - 0.0).abs() < f64::EPSILON);
        assert_eq!(score.controls_stale, 1);
        assert_eq!(score.controls_covered, 0);
    }

    #[test]
    fn partial_counts_as_covered_not_passing() {
        let controls = vec![make_control("C1", true)];
        let now = Utc::now();
        let evidence = vec![make_evidence(
            "C1",
            EvidenceResult::Partial,
            now,
            EvidenceType::ChaosExperiment,
        )];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert!((score.score - 0.0).abs() < f64::EPSILON);
        assert_eq!(score.controls_covered, 1);
        assert_eq!(score.controls_passing, 0);
    }

    #[test]
    fn best_fresh_result_wins() {
        let controls = vec![make_control("C1", true)];
        let now = Utc::now();
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
        assert!((score.score - 100.0).abs() < f64::EPSILON);
        assert_eq!(score.controls_passing, 1);
    }

    #[test]
    fn non_testing_relevant_controls_excluded() {
        let controls = vec![make_control("C1", true), make_control("C2", false)];
        let now = Utc::now();
        let evidence = vec![make_evidence(
            "C1",
            EvidenceResult::Pass,
            now,
            EvidenceType::ChaosExperiment,
        )];
        let score =
            compute_entity_framework_score(&entity_id(), &fw_id(), &controls, &evidence, now);
        assert_eq!(score.controls_total, 1);
        assert!((score.score - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mixed_results_correct_score() {
        let controls = vec![
            make_control("C1", true),
            make_control("C2", true),
            make_control("C3", true),
            make_control("C4", true),
        ];
        let now = Utc::now();
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
        assert!((score.score - 50.0).abs() < f64::EPSILON);
        assert_eq!(score.controls_total, 4);
        assert_eq!(score.controls_covered, 3);
        assert_eq!(score.controls_passing, 2);
        assert_eq!(score.badge, BadgeTier::Bronze);
    }

    #[test]
    fn no_controls_scores_zero() {
        let now = Utc::now();
        let score = compute_entity_framework_score(&entity_id(), &fw_id(), &[], &[], now);
        assert!((score.score - 0.0).abs() < f64::EPSILON);
        assert_eq!(score.controls_total, 0);
    }
}
