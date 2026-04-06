use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use super::ComplianceScore;
use crate::models::control::{Control, ControlId};
use crate::models::evidence::{Evidence, EvidenceResult};
use crate::models::framework::FrameworkId;
use crate::models::mapping::{Confidence, ControlMapping};
use crate::models::org::EntityId;

/// Enhance a direct score with cross-framework evidence via control mappings.
///
/// For each testing-relevant control in the target framework that has no direct
/// fresh evidence, check if a mapped control in another framework has fresh
/// passing evidence. Only mappings with confidence >= Medium are considered.
/// Direct evidence always takes precedence.
#[must_use]
pub fn enhance_with_mappings(
    direct_score: &ComplianceScore,
    entity_id: &EntityId,
    controls: &[Control],
    mappings: &[ControlMapping],
    all_evidence: &[Evidence],
    now: DateTime<Utc>,
) -> ComplianceScore {
    let framework_id = &direct_score.framework_id;

    let relevant_controls: Vec<&Control> = controls
        .iter()
        .filter(|c| c.framework_id == *framework_id && c.testing_relevant)
        .collect();

    if relevant_controls.is_empty() {
        return direct_score.clone();
    }

    // Build set of controls that already have direct fresh evidence
    let controls_with_direct: HashSet<&ControlId> = build_direct_coverage(
        entity_id,
        &relevant_controls,
        all_evidence,
        now,
    );

    // Build map: control_id -> set of mapped source control_ids (confidence >= Medium)
    let mapping_index = build_mapping_index(framework_id, mappings);

    // Build set of control_ids with fresh passing evidence (any framework, this entity)
    let passing_controls = build_passing_controls(entity_id, all_evidence, now);

    let mut controls_passing = direct_score.controls_passing;
    let mut controls_covered = direct_score.controls_covered;

    for ctrl in &relevant_controls {
        if controls_with_direct.contains(&ctrl.control_id) {
            continue;
        }

        if let Some(mapped_sources) = mapping_index.get(&ctrl.control_id) {
            if mapped_sources.iter().any(|src| passing_controls.contains(src)) {
                controls_passing += 1;
                controls_covered += 1;
            }
        }
    }

    ComplianceScore::new(
        framework_id.clone(),
        direct_score.controls_total,
        controls_covered,
        controls_passing,
        direct_score.controls_stale,
    )
}

/// Find controls that have any direct fresh evidence for this entity.
fn build_direct_coverage<'a>(
    entity_id: &EntityId,
    relevant_controls: &[&'a Control],
    evidence: &[Evidence],
    now: DateTime<Utc>,
) -> HashSet<&'a ControlId> {
    let relevant_ids: HashSet<&ControlId> = relevant_controls
        .iter()
        .map(|c| &c.control_id)
        .collect();

    evidence
        .iter()
        .filter(|ev| {
            ev.entity_id == *entity_id
                && ev.expires_at > now
                && relevant_ids.contains(&ev.control_id)
        })
        .map(|ev| relevant_ids.get(&ev.control_id).copied().unwrap())
        .collect()
}

/// Build index: `target_control_id` -> set of `source_control_ids` from qualified mappings.
fn build_mapping_index<'a>(
    framework_id: &FrameworkId,
    mappings: &'a [ControlMapping],
) -> HashMap<&'a ControlId, Vec<&'a ControlId>> {
    let mut index: HashMap<&ControlId, Vec<&ControlId>> = HashMap::new();

    for m in mappings {
        if m.confidence == Confidence::Low {
            continue;
        }
        // If target_control belongs to our framework, map from source
        // We check by convention: mappings where target is in our framework
        // The caller must provide mappings where target_control is in the target framework
        index
            .entry(&m.target_control)
            .or_default()
            .push(&m.source_control);
    }

    // Filter: only keep entries where the target is actually in our framework
    // We rely on the caller to only pass relevant mappings, but for safety
    // we don't filter here since we don't have framework_id on ControlId
    let _ = framework_id;
    index
}

/// Find all `ControlId`s that have fresh passing evidence for this entity.
fn build_passing_controls<'a>(
    entity_id: &EntityId,
    evidence: &'a [Evidence],
    now: DateTime<Utc>,
) -> HashSet<&'a ControlId> {
    evidence
        .iter()
        .filter(|ev| {
            ev.entity_id == *entity_id
                && ev.expires_at > now
                && ev.result == EvidenceResult::Pass
        })
        .map(|ev| &ev.control_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::control::Severity;
    use crate::models::evidence::{EvidenceId, EvidenceType, SourceSystem};
    use crate::models::freshness::compute_expires_at;
    use crate::models::mapping::{MappingDirection, MappingProvenance, MappingRelationship};
    use crate::scoring::engine::compute_entity_framework_score;

    fn fw(id: &str) -> FrameworkId {
        FrameworkId::new(id).unwrap()
    }

    fn ctrl_id(id: &str) -> ControlId {
        ControlId::new(id).unwrap()
    }

    fn entity() -> EntityId {
        EntityId::new("team-alpha")
    }

    fn make_control(id: &str, framework: &str) -> Control {
        Control::builder(ctrl_id(id), fw(framework), format!("Control {id}"))
            .testing_relevant(true)
            .severity(Severity::High)
            .build()
    }

    fn make_evidence(
        control_id: &str,
        result: EvidenceResult,
        observed_at: DateTime<Utc>,
    ) -> Evidence {
        let et = EvidenceType::ChaosExperiment;
        Evidence {
            evidence_id: EvidenceId::new(),
            entity_id: entity(),
            control_id: ctrl_id(control_id),
            evidence_type: et,
            source_system: SourceSystem::new("tumult"),
            result,
            score: None,
            metadata: serde_json::Value::Null,
            observed_at,
            expires_at: compute_expires_at(observed_at, &et),
        }
    }

    fn make_mapping(
        source: &str,
        target: &str,
        confidence: Confidence,
    ) -> ControlMapping {
        ControlMapping::new(
            ctrl_id(source),
            ctrl_id(target),
            MappingRelationship::Equivalent,
            confidence,
            MappingDirection::Bidirectional,
            MappingProvenance::NistOlir,
        )
    }

    #[test]
    fn mapped_evidence_fills_gap() {
        let now = Utc::now();
        // DORA has C1, C2. Entity has direct evidence only for C1.
        // NIST has N1 mapped to DORA C2 (High confidence). Entity passes N1.
        let controls = vec![
            make_control("C1", "DORA"),
            make_control("C2", "DORA"),
            make_control("N1", "NIST"),
        ];
        let evidence = vec![
            make_evidence("C1", EvidenceResult::Pass, now),
            make_evidence("N1", EvidenceResult::Pass, now),
        ];
        let mappings = vec![make_mapping("N1", "C2", Confidence::High)];

        let direct = compute_entity_framework_score(
            &entity(), &fw("DORA"), &controls, &evidence, now,
        );
        assert_eq!(direct.controls_passing, 1);
        assert!((direct.score - 50.0).abs() < f64::EPSILON);

        let enhanced = enhance_with_mappings(
            &direct, &entity(), &controls, &mappings, &evidence, now,
        );
        assert_eq!(enhanced.controls_passing, 2);
        assert!((enhanced.score - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn direct_failing_overrides_mapped_passing() {
        let now = Utc::now();
        let controls = vec![
            make_control("C1", "DORA"),
            make_control("N1", "NIST"),
        ];
        // C1 has direct FAILING evidence. N1 passes and maps to C1.
        let evidence = vec![
            make_evidence("C1", EvidenceResult::Fail, now),
            make_evidence("N1", EvidenceResult::Pass, now),
        ];
        let mappings = vec![make_mapping("N1", "C1", Confidence::High)];

        let direct = compute_entity_framework_score(
            &entity(), &fw("DORA"), &controls, &evidence, now,
        );
        assert_eq!(direct.controls_passing, 0);

        let enhanced = enhance_with_mappings(
            &direct, &entity(), &controls, &mappings, &evidence, now,
        );
        // Direct evidence exists — mapping should NOT override
        assert_eq!(enhanced.controls_passing, 0);
    }

    #[test]
    fn low_confidence_mappings_ignored() {
        let now = Utc::now();
        let controls = vec![
            make_control("C1", "DORA"),
            make_control("N1", "NIST"),
        ];
        let evidence = vec![make_evidence("N1", EvidenceResult::Pass, now)];
        let mappings = vec![make_mapping("N1", "C1", Confidence::Low)];

        let direct = compute_entity_framework_score(
            &entity(), &fw("DORA"), &controls, &evidence, now,
        );
        let enhanced = enhance_with_mappings(
            &direct, &entity(), &controls, &mappings, &evidence, now,
        );
        assert_eq!(enhanced.controls_passing, 0);
    }

    #[test]
    fn medium_confidence_mappings_accepted() {
        let now = Utc::now();
        let controls = vec![
            make_control("C1", "DORA"),
            make_control("N1", "NIST"),
        ];
        let evidence = vec![make_evidence("N1", EvidenceResult::Pass, now)];
        let mappings = vec![make_mapping("N1", "C1", Confidence::Medium)];

        let direct = compute_entity_framework_score(
            &entity(), &fw("DORA"), &controls, &evidence, now,
        );
        let enhanced = enhance_with_mappings(
            &direct, &entity(), &controls, &mappings, &evidence, now,
        );
        assert_eq!(enhanced.controls_passing, 1);
    }

    #[test]
    fn score_capped_at_100() {
        let now = Utc::now();
        let controls = vec![
            make_control("C1", "DORA"),
            make_control("N1", "NIST"),
        ];
        let evidence = vec![
            make_evidence("C1", EvidenceResult::Pass, now),
            make_evidence("N1", EvidenceResult::Pass, now),
        ];
        let mappings = vec![make_mapping("N1", "C1", Confidence::High)];

        let direct = compute_entity_framework_score(
            &entity(), &fw("DORA"), &controls, &evidence, now,
        );
        let enhanced = enhance_with_mappings(
            &direct, &entity(), &controls, &mappings, &evidence, now,
        );
        assert!(enhanced.score <= 100.0);
    }
}
