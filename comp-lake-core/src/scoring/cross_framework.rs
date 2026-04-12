use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use super::ComplianceScore;
use crate::models::control::{Control, ControlId};
use crate::models::evidence::{Evidence, EvidenceResult};
use crate::models::mapping::{Confidence, ControlMapping};
use crate::models::org::EntityId;

/// Enhance a direct score with cross-framework evidence via control mappings.
///
/// For each testing-relevant control in the target framework that has no direct
/// fresh evidence, check if a mapped control in another framework has fresh
/// passing evidence. Only mappings with confidence >= Medium are considered.
/// Direct evidence always takes precedence.
///
/// # Propagation depth
///
/// Propagation is **one-hop only**. Evidence for control A fills gaps in
/// directly-mapped control B, but does **not** transitively fill gaps in
/// controls mapped from B (i.e. no chain A→B→C). Callers who need multi-hop
/// propagation must call this function iteratively or pre-compute the transitive
/// closure of mappings beforehand.
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
    let controls_with_direct: HashSet<&ControlId> =
        build_direct_coverage(entity_id, &relevant_controls, all_evidence, now);

    // Build map: control_id -> set of mapped source control_ids (confidence >= Medium)
    let mapping_index = build_mapping_index(mappings);

    // Build set of control_ids with fresh passing evidence (any framework, this entity)
    let passing_controls = build_passing_controls(entity_id, all_evidence, now);

    let mut controls_passing = direct_score.controls_passing;
    let mut controls_covered = direct_score.controls_covered;

    for ctrl in &relevant_controls {
        if controls_with_direct.contains(&ctrl.control_id) {
            continue;
        }

        if let Some(mapped_sources) = mapping_index.get(&ctrl.control_id) {
            if mapped_sources
                .iter()
                .any(|src| passing_controls.contains(src))
            {
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
    let relevant_ids: HashSet<&ControlId> =
        relevant_controls.iter().map(|c| &c.control_id).collect();

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
fn build_mapping_index(mappings: &[ControlMapping]) -> HashMap<&ControlId, Vec<&ControlId>> {
    let mut index: HashMap<&ControlId, Vec<&ControlId>> = HashMap::new();

    for m in mappings {
        if m.confidence == Confidence::Low {
            continue;
        }
        index
            .entry(&m.target_control)
            .or_default()
            .push(&m.source_control);
    }

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
            ev.entity_id == *entity_id && ev.expires_at > now && ev.result == EvidenceResult::Pass
        })
        .map(|ev| &ev.control_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::control::Severity;
    use crate::models::evidence::{EvidenceId, EvidenceType, SourceSystem};
    use crate::models::framework::FrameworkId;
    use crate::models::freshness::compute_expires_at;
    use crate::models::mapping::{MappingDirection, MappingProvenance, MappingRelationship};
    use crate::scoring::engine::compute_entity_framework_score;
    use chrono::TimeZone;

    const SCORE_TOLERANCE: f64 = 1e-10;

    fn assert_score_eq(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < SCORE_TOLERANCE,
            "score mismatch: expected {expected}, got {actual}"
        );
    }

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 12, 12, 0, 0)
            .single()
            .expect("valid fixed timestamp")
    }

    fn fw(id: &str) -> FrameworkId {
        FrameworkId::new(id).expect("valid framework ID")
    }

    fn ctrl_id(id: &str) -> ControlId {
        ControlId::new(id).expect("valid control ID")
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

    fn make_mapping(source: &str, target: &str, confidence: Confidence) -> ControlMapping {
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
        let now = fixed_now();
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

        let direct =
            compute_entity_framework_score(&entity(), &fw("DORA"), &controls, &evidence, now);
        assert_eq!(direct.controls_passing, 1);
        assert_score_eq(direct.score, 50.0);

        let enhanced =
            enhance_with_mappings(&direct, &entity(), &controls, &mappings, &evidence, now);
        assert_eq!(enhanced.controls_passing, 2);
        assert_score_eq(enhanced.score, 100.0);
    }

    #[test]
    fn direct_failing_overrides_mapped_passing() {
        let now = fixed_now();
        let controls = vec![make_control("C1", "DORA"), make_control("N1", "NIST")];
        let evidence = vec![
            make_evidence("C1", EvidenceResult::Fail, now),
            make_evidence("N1", EvidenceResult::Pass, now),
        ];
        let mappings = vec![make_mapping("N1", "C1", Confidence::High)];

        let direct =
            compute_entity_framework_score(&entity(), &fw("DORA"), &controls, &evidence, now);
        assert_eq!(direct.controls_passing, 0);

        let enhanced =
            enhance_with_mappings(&direct, &entity(), &controls, &mappings, &evidence, now);
        assert_eq!(enhanced.controls_passing, 0);
    }

    #[test]
    fn low_confidence_mappings_ignored() {
        let now = fixed_now();
        let controls = vec![make_control("C1", "DORA"), make_control("N1", "NIST")];
        let evidence = vec![make_evidence("N1", EvidenceResult::Pass, now)];
        let mappings = vec![make_mapping("N1", "C1", Confidence::Low)];

        let direct =
            compute_entity_framework_score(&entity(), &fw("DORA"), &controls, &evidence, now);
        let enhanced =
            enhance_with_mappings(&direct, &entity(), &controls, &mappings, &evidence, now);
        assert_eq!(enhanced.controls_passing, 0);
    }

    #[test]
    fn medium_confidence_mappings_accepted() {
        let now = fixed_now();
        let controls = vec![make_control("C1", "DORA"), make_control("N1", "NIST")];
        let evidence = vec![make_evidence("N1", EvidenceResult::Pass, now)];
        let mappings = vec![make_mapping("N1", "C1", Confidence::Medium)];

        let direct =
            compute_entity_framework_score(&entity(), &fw("DORA"), &controls, &evidence, now);
        let enhanced =
            enhance_with_mappings(&direct, &entity(), &controls, &mappings, &evidence, now);
        assert_eq!(enhanced.controls_passing, 1);
    }

    #[test]
    fn score_capped_at_100() {
        let now = fixed_now();
        let controls = vec![make_control("C1", "DORA"), make_control("N1", "NIST")];
        let evidence = vec![
            make_evidence("C1", EvidenceResult::Pass, now),
            make_evidence("N1", EvidenceResult::Pass, now),
        ];
        let mappings = vec![make_mapping("N1", "C1", Confidence::High)];

        let direct =
            compute_entity_framework_score(&entity(), &fw("DORA"), &controls, &evidence, now);
        let enhanced =
            enhance_with_mappings(&direct, &entity(), &controls, &mappings, &evidence, now);
        assert_score_eq(enhanced.score, 100.0);
    }

    #[test]
    fn known_graph_exact_enhanced_score() {
        let now = fixed_now();
        let controls = vec![
            make_control("C1", "DORA"),
            make_control("C2", "DORA"),
            make_control("C3", "DORA"),
            make_control("C4", "DORA"),
            make_control("N1", "NIST"),
            make_control("N2", "NIST"),
        ];
        let evidence = vec![
            make_evidence("C1", EvidenceResult::Pass, now),
            make_evidence("C2", EvidenceResult::Fail, now),
            make_evidence("N1", EvidenceResult::Pass, now),
            make_evidence("N2", EvidenceResult::Pass, now),
        ];
        let mappings = vec![
            make_mapping("N1", "C3", Confidence::High),
            make_mapping("N2", "C4", Confidence::Medium),
        ];

        let direct =
            compute_entity_framework_score(&entity(), &fw("DORA"), &controls, &evidence, now);
        assert_eq!(direct.controls_passing, 1);
        assert_score_eq(direct.score, 25.0);

        let enhanced =
            enhance_with_mappings(&direct, &entity(), &controls, &mappings, &evidence, now);
        assert_eq!(enhanced.controls_passing, 3);
        assert_eq!(enhanced.controls_covered, 4);
        assert_score_eq(enhanced.score, 75.0);
    }

    #[test]
    fn direct_passing_overrides_mapped_failing() {
        let now = fixed_now();
        let controls = vec![make_control("C1", "DORA"), make_control("N1", "NIST")];
        let evidence = vec![
            make_evidence("C1", EvidenceResult::Pass, now),
            make_evidence("N1", EvidenceResult::Fail, now),
        ];
        let mappings = vec![make_mapping("N1", "C1", Confidence::High)];

        let direct =
            compute_entity_framework_score(&entity(), &fw("DORA"), &controls, &evidence, now);
        let enhanced =
            enhance_with_mappings(&direct, &entity(), &controls, &mappings, &evidence, now);
        assert_eq!(enhanced.controls_passing, 1);
        assert_score_eq(enhanced.score, 100.0);
    }

    #[test]
    fn no_mappings_returns_direct_score() {
        let now = fixed_now();
        let controls = vec![make_control("C1", "DORA")];
        let evidence = vec![make_evidence("C1", EvidenceResult::Pass, now)];

        let direct =
            compute_entity_framework_score(&entity(), &fw("DORA"), &controls, &evidence, now);
        let enhanced = enhance_with_mappings(&direct, &entity(), &controls, &[], &evidence, now);
        assert_eq!(enhanced.controls_passing, direct.controls_passing);
        assert_score_eq(enhanced.score, direct.score);
    }

    #[test]
    fn empty_controls_returns_direct_clone() {
        let now = fixed_now();
        let direct = ComplianceScore::new(fw("DORA"), 0, 0, 0, 0);
        let enhanced = enhance_with_mappings(&direct, &entity(), &[], &[], &[], now);
        assert_eq!(enhanced.controls_total, 0);
        assert_score_eq(enhanced.score, 0.0);
    }

    #[test]
    fn empty_evidence_with_mappings_no_enhancement() {
        let now = fixed_now();
        let controls = vec![make_control("C1", "DORA"), make_control("N1", "NIST")];
        let mappings = vec![make_mapping("N1", "C1", Confidence::High)];

        let direct = compute_entity_framework_score(&entity(), &fw("DORA"), &controls, &[], now);
        let enhanced = enhance_with_mappings(&direct, &entity(), &controls, &mappings, &[], now);
        assert_eq!(enhanced.controls_passing, 0);
        assert_score_eq(enhanced.score, 0.0);
    }

    /// Chain propagation is one-hop only: A->B->C does NOT propagate A's evidence to C.
    ///
    /// Given mappings A->B and B->C (both High confidence), passing evidence for A
    /// fills B's gap but does NOT fill C's gap. This is the documented design decision.
    #[test]
    fn chain_propagation_is_one_hop_only() {
        let now = fixed_now();
        // DORA has B and C. NIST has A.
        // A->B mapping: A(NIST) passes, fills B(DORA) gap.
        // B->C mapping: would require B to have evidence to fill C — but enhance_with_mappings
        // only indexes one level of source->target. B having no direct evidence means C is not filled.
        let controls = vec![
            make_control("B", "DORA"),
            make_control("C", "DORA"),
            make_control("A", "NIST"),
        ];
        let evidence = vec![make_evidence("A", EvidenceResult::Pass, now)];
        let mappings = vec![
            make_mapping("A", "B", Confidence::High),
            make_mapping("B", "C", Confidence::High),
        ];

        let direct =
            compute_entity_framework_score(&entity(), &fw("DORA"), &controls, &evidence, now);
        assert_eq!(direct.controls_passing, 0, "no direct DORA evidence");

        let enhanced =
            enhance_with_mappings(&direct, &entity(), &controls, &mappings, &evidence, now);

        // A's passing evidence fills B (one hop). C is NOT filled (no chain transitive hop).
        assert_eq!(
            enhanced.controls_passing, 1,
            "only B filled (one-hop from A); C must not be filled via chain B->C"
        );
        assert_score_eq(enhanced.score, 50.0);
    }

    /// A Bidirectional mapping propagates in both directions independently.
    ///
    /// The same mapping relationship D1(DORA)<->N1(NIST) should fill D1's gap
    /// when N1 has evidence (scoring DORA), and fill N1's gap when D1 has
    /// evidence (scoring NIST). Two separate mappings are used since the index
    /// is directional (target->source).
    #[test]
    fn bidirectional_propagates_in_both_directions() {
        let now = fixed_now();

        let dora_ctrl = make_control("D1", "DORA");
        let nist_ctrl = make_control("N1", "NIST");
        let all_controls = vec![dora_ctrl.clone(), nist_ctrl.clone()];

        // N1 passes; D1 has no evidence
        let n1_passes = vec![make_evidence("N1", EvidenceResult::Pass, now)];
        // D1 passes; N1 has no evidence
        let d1_passes = vec![make_evidence("D1", EvidenceResult::Pass, now)];

        // Direction 1: N1->D1 (source=N1, target=D1)
        let mapping_n1_to_d1 = make_mapping("N1", "D1", Confidence::High);
        // Direction 2: D1->N1 (source=D1, target=N1)
        let mapping_d1_to_n1 = make_mapping("D1", "N1", Confidence::High);

        // === Scoring DORA: N1's evidence should fill D1's gap ===
        let direct_dora =
            compute_entity_framework_score(&entity(), &fw("DORA"), &all_controls, &n1_passes, now);
        assert_eq!(direct_dora.controls_passing, 0, "no direct DORA evidence");

        let enhanced_dora = enhance_with_mappings(
            &direct_dora,
            &entity(),
            &all_controls,
            &[mapping_n1_to_d1],
            &n1_passes,
            now,
        );
        assert_eq!(
            enhanced_dora.controls_passing, 1,
            "N1's evidence should fill D1 gap (scoring DORA)"
        );
        assert_score_eq(enhanced_dora.score, 100.0);

        // === Scoring NIST: D1's evidence should fill N1's gap ===
        let direct_nist =
            compute_entity_framework_score(&entity(), &fw("NIST"), &all_controls, &d1_passes, now);
        assert_eq!(direct_nist.controls_passing, 0, "no direct NIST evidence");

        let enhanced_nist = enhance_with_mappings(
            &direct_nist,
            &entity(),
            &all_controls,
            &[mapping_d1_to_n1],
            &d1_passes,
            now,
        );
        assert_eq!(
            enhanced_nist.controls_passing, 1,
            "D1's evidence should fill N1 gap (scoring NIST)"
        );
        assert_score_eq(enhanced_nist.score, 100.0);
    }
}
