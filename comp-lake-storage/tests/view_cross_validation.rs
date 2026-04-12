//! Cross-validation tests: SQL views vs Rust scoring engine.
//!
//! Each test populates `DuckDB` with known data, queries the corresponding view,
//! then runs the Rust scoring engine on the same data and asserts equivalence.

use chrono::{Duration, Utc};
use comp_lake_core::models::control::{Control, ControlFamily, ControlId, Severity};
use comp_lake_core::models::evidence::{
    Evidence, EvidenceId, EvidenceResult, EvidenceType, SourceSystem,
};
use comp_lake_core::models::framework::{Framework, FrameworkId, HarvestSource, Region};
use comp_lake_core::models::freshness::compute_expires_at;
use comp_lake_core::models::mapping::{
    Confidence, ControlMapping, MappingDirection, MappingProvenance, MappingRelationship,
};
use comp_lake_core::models::org::{EntityId, EntityType, OrgEntity};
use comp_lake_core::scoring::engine::compute_entity_framework_score;
use comp_lake_storage::store::CompLakeStore;
use comp_lake_storage::views;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup_store() -> CompLakeStore {
    let store = CompLakeStore::in_memory().expect("in-memory store");
    views::create_views(store.conn()).expect("views created");
    store
}

fn dora_fw() -> Framework {
    Framework::builder(FrameworkId::new("DORA").expect("valid id"), "DORA")
        .version("2022/2554")
        .region(Region::Eu)
        .authority("EU/EP")
        .harvest_source(HarvestSource::Manual)
        .build()
}

fn nist_fw() -> Framework {
    Framework::builder(FrameworkId::new("NIST").expect("valid id"), "NIST CSF")
        .version("2.0")
        .region(Region::Us)
        .authority("NIST")
        .harvest_source(HarvestSource::OscalGithub)
        .build()
}

fn make_control(id: &str, fw: &str, severity: Severity) -> Control {
    Control::builder(
        ControlId::new(id).expect("valid ctrl id"),
        FrameworkId::new(fw).expect("valid fw id"),
        format!("Control {id}"),
    )
    .severity(severity)
    .family(ControlFamily::new("Testing"))
    .testing_relevant(true)
    .build()
}

fn make_evidence(
    entity: &str,
    ctrl_id: &str,
    result: EvidenceResult,
    observed_at: chrono::DateTime<Utc>,
    evidence_type: EvidenceType,
) -> Evidence {
    Evidence {
        evidence_id: EvidenceId::new(),
        entity_id: EntityId::new(entity),
        control_id: ControlId::new(ctrl_id).expect("valid ctrl id"),
        evidence_type,
        source_system: SourceSystem::new("tumult"),
        result,
        score: None,
        metadata: serde_json::Value::Null,
        observed_at,
        expires_at: compute_expires_at(observed_at, &evidence_type),
    }
}

fn project_entity(id: &str, name: &str, parent: Option<&str>) -> OrgEntity {
    OrgEntity::new(
        EntityId::new(id),
        EntityType::Project,
        name,
        parent.map(EntityId::new),
    )
}

fn team_entity(id: &str, name: &str, parent: Option<&str>) -> OrgEntity {
    OrgEntity::new(
        EntityId::new(id),
        EntityType::Team,
        name,
        parent.map(EntityId::new),
    )
}

/// Seed a standard dataset: DORA framework, 4 controls, 1 entity, mixed evidence.
/// Returns the controls and evidence vectors for Rust-side scoring.
fn seed_standard(store: &CompLakeStore) -> (Vec<Control>, Vec<Evidence>, chrono::DateTime<Utc>) {
    store.upsert_framework(&dora_fw()).expect("fw insert");

    let controls = vec![
        make_control("C1", "DORA", Severity::High),
        make_control("C2", "DORA", Severity::High),
        make_control("C3", "DORA", Severity::Moderate),
        make_control("C4", "DORA", Severity::Low),
    ];
    for c in &controls {
        store.upsert_control(c).expect("ctrl insert");
    }

    let entity = project_entity("proj-1", "Project One", None);
    store.upsert_org_entity(&entity).expect("entity insert");

    let now = Utc::now();
    let et = EvidenceType::ChaosExperiment;

    // C1=Pass, C2=Partial, C3=Fail, C4=no evidence
    let evidence = vec![
        make_evidence("proj-1", "C1", EvidenceResult::Pass, now, et),
        make_evidence("proj-1", "C2", EvidenceResult::Partial, now, et),
        make_evidence("proj-1", "C3", EvidenceResult::Fail, now, et),
    ];
    for ev in &evidence {
        store.upsert_evidence(ev).expect("evidence insert");
    }

    (controls, evidence, now)
}

// ---------------------------------------------------------------------------
// T4.3: v_scores cross-validation
// ---------------------------------------------------------------------------

#[test]
fn v_scores_matches_engine_for_standard_dataset() {
    let store = setup_store();
    let (controls, evidence, now) = seed_standard(&store);

    // SQL view result
    let (view_passing, view_total, view_score): (i64, i64, f64) = store
        .conn()
        .query_row(
            "SELECT controls_passing, controls_total, score FROM v_scores \
             WHERE entity_id = 'proj-1' AND framework_id = 'DORA'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("v_scores query");

    // Rust engine result
    let engine = compute_entity_framework_score(
        &EntityId::new("proj-1"),
        &FrameworkId::new("DORA").expect("valid"),
        &controls,
        &evidence,
        now,
    );

    #[allow(clippy::cast_possible_wrap)]
    {
        assert_eq!(
            engine.controls_total as i64, view_total,
            "controls_total mismatch"
        );
        assert_eq!(
            engine.controls_passing as i64, view_passing,
            "controls_passing mismatch"
        );
    }
    assert!(
        (engine.score - view_score).abs() < 1e-6,
        "score mismatch: engine={}, view={}",
        engine.score,
        view_score,
    );
}

#[test]
fn v_scores_all_passing() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw");

    let controls = vec![
        make_control("C1", "DORA", Severity::High),
        make_control("C2", "DORA", Severity::High),
    ];
    for c in &controls {
        store.upsert_control(c).expect("ctrl");
    }

    let entity = project_entity("proj-1", "Project One", None);
    store.upsert_org_entity(&entity).expect("entity");

    let now = Utc::now();
    let et = EvidenceType::ChaosExperiment;
    let evidence = vec![
        make_evidence("proj-1", "C1", EvidenceResult::Pass, now, et),
        make_evidence("proj-1", "C2", EvidenceResult::Pass, now, et),
    ];
    for ev in &evidence {
        store.upsert_evidence(ev).expect("ev");
    }

    let view_score: f64 = store
        .conn()
        .query_row(
            "SELECT score FROM v_scores \
             WHERE entity_id = 'proj-1' AND framework_id = 'DORA'",
            [],
            |row| row.get(0),
        )
        .expect("query");

    let engine = compute_entity_framework_score(
        &EntityId::new("proj-1"),
        &FrameworkId::new("DORA").expect("valid"),
        &controls,
        &evidence,
        now,
    );

    assert!(
        (view_score - 100.0).abs() < f64::EPSILON,
        "view should be 100.0, got {view_score}"
    );
    assert!(
        (engine.score - view_score).abs() < f64::EPSILON,
        "engine/view mismatch"
    );
}

#[test]
fn v_scores_badge_matches_engine() {
    let store = setup_store();
    let (controls, evidence, now) = seed_standard(&store);

    let view_badge: String = store
        .conn()
        .query_row(
            "SELECT badge FROM v_scores \
             WHERE entity_id = 'proj-1' AND framework_id = 'DORA'",
            [],
            |row| row.get(0),
        )
        .expect("badge query");

    let engine = compute_entity_framework_score(
        &EntityId::new("proj-1"),
        &FrameworkId::new("DORA").expect("valid"),
        &controls,
        &evidence,
        now,
    );

    let engine_badge = format!("{:?}", engine.badge);
    assert_eq!(
        view_badge, engine_badge,
        "badge mismatch: view={view_badge}, engine={engine_badge}"
    );
}

#[test]
fn v_scores_no_evidence_returns_zero() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw");
    store
        .upsert_control(&make_control("C1", "DORA", Severity::High))
        .expect("ctrl");
    store
        .upsert_org_entity(&project_entity("proj-1", "P1", None))
        .expect("entity");

    // No evidence inserted
    let view_score: Option<f64> = store
        .conn()
        .query_row(
            "SELECT score FROM v_scores \
             WHERE entity_id = 'proj-1' AND framework_id = 'DORA'",
            [],
            |row| row.get(0),
        )
        .expect("query");

    let engine = compute_entity_framework_score(
        &EntityId::new("proj-1"),
        &FrameworkId::new("DORA").expect("valid"),
        &[make_control("C1", "DORA", Severity::High)],
        &[],
        Utc::now(),
    );

    // v_scores returns 0.0 (not NULL) when there is a control but no passing evidence:
    // ROUND(100.0 * 0 / NULLIF(1, 0), 1) = 0.0.
    // NULL is only returned when controls_total = 0 (no controls at all).
    assert_eq!(
        view_score,
        Some(0.0),
        "v_scores should return 0.0 (not NULL) for an entity with controls but no evidence"
    );
    assert!(
        (engine.score - view_score.unwrap()).abs() < f64::EPSILON,
        "engine/view score mismatch for zero-evidence entity: engine={}, view={}",
        engine.score,
        view_score.unwrap(),
    );
}

#[test]
fn v_scores_stale_evidence_excluded() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw");
    store
        .upsert_control(&make_control("C1", "DORA", Severity::High))
        .expect("ctrl");
    store
        .upsert_org_entity(&project_entity("proj-1", "P1", None))
        .expect("entity");

    let now = Utc::now();
    // Evidence observed 200 days ago (ChaosExperiment = 90d freshness => stale)
    let old = now - Duration::days(200);
    let ev = make_evidence(
        "proj-1",
        "C1",
        EvidenceResult::Pass,
        old,
        EvidenceType::ChaosExperiment,
    );
    store.upsert_evidence(&ev).expect("ev");

    let view_stale: i64 = store
        .conn()
        .query_row(
            "SELECT controls_stale FROM v_scores \
             WHERE entity_id = 'proj-1' AND framework_id = 'DORA'",
            [],
            |row| row.get(0),
        )
        .expect("query");

    let engine = compute_entity_framework_score(
        &EntityId::new("proj-1"),
        &FrameworkId::new("DORA").expect("valid"),
        &[make_control("C1", "DORA", Severity::High)],
        &[ev],
        now,
    );

    assert_eq!(view_stale, 1, "view should report 1 stale control");
    #[allow(clippy::cast_possible_wrap)]
    {
        assert_eq!(
            engine.controls_stale as i64, view_stale,
            "stale count mismatch"
        );
    }
}

// ---------------------------------------------------------------------------
// T4.3: v_rollup_scores cross-validation
// ---------------------------------------------------------------------------

#[test]
fn v_rollup_scores_team_averages_projects() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw");

    // 2 controls
    let controls = vec![
        make_control("C1", "DORA", Severity::High),
        make_control("C2", "DORA", Severity::High),
    ];
    for c in &controls {
        store.upsert_control(c).expect("ctrl");
    }

    // Hierarchy: team-a -> proj-1, proj-2
    store
        .upsert_org_entity(&team_entity("team-a", "Team A", None))
        .expect("team");
    store
        .upsert_org_entity(&project_entity("proj-1", "P1", Some("team-a")))
        .expect("p1");
    store
        .upsert_org_entity(&project_entity("proj-2", "P2", Some("team-a")))
        .expect("p2");

    let now = Utc::now();
    let et = EvidenceType::ChaosExperiment;

    // proj-1: both pass => 100%
    for ctrl_id in ["C1", "C2"] {
        let ev = make_evidence("proj-1", ctrl_id, EvidenceResult::Pass, now, et);
        store.upsert_evidence(&ev).expect("ev");
    }
    // proj-2: C1 pass only => 50%
    let ev = make_evidence("proj-2", "C1", EvidenceResult::Pass, now, et);
    store.upsert_evidence(&ev).expect("ev");

    // Query rollup view for team-a
    let result = store.conn().query_row(
        "SELECT score FROM v_rollup_scores \
         WHERE entity_id = 'team-a' AND framework_id = 'DORA' AND entity_type = 'Team'",
        [],
        |row| row.get::<_, f64>(0),
    );

    match result {
        Ok(team_score) => {
            // Should be avg(100, 50) = 75.0
            assert!(
                (team_score - 75.0).abs() < 0.01,
                "team rollup expected ~75, got {team_score}"
            );
        }
        Err(duckdb::Error::QueryReturnedNoRows) => {
            // The recursive CTE only starts from Project entities,
            // so Team rollup should exist if the CTE is correct.
            panic!("v_rollup_scores returned no rows for team-a");
        }
        Err(e) => panic!("unexpected error: {e}"),
    }
}

// ---------------------------------------------------------------------------
// T4.3: v_coverage_gaps priority ordering
// ---------------------------------------------------------------------------

#[test]
fn v_coverage_gaps_high_never_tested_before_moderate_stale() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw");

    // C1: High severity, never tested
    // C2: Moderate severity, stale evidence
    store
        .upsert_control(&make_control("C1", "DORA", Severity::High))
        .expect("c1");
    store
        .upsert_control(&make_control("C2", "DORA", Severity::Moderate))
        .expect("c2");

    store
        .upsert_org_entity(&project_entity("proj-1", "P1", None))
        .expect("entity");

    // Add stale evidence for C2
    let now = Utc::now();
    let old = now - Duration::days(200);
    let ev = make_evidence(
        "proj-1",
        "C2",
        EvidenceResult::Pass,
        old,
        EvidenceType::ChaosExperiment,
    );
    store.upsert_evidence(&ev).expect("ev");

    // Query gaps ordered by priority_rank
    let mut stmt = store
        .conn()
        .prepare(
            "SELECT control_id, gap_reason, priority_rank FROM v_coverage_gaps \
             WHERE entity_id = 'proj-1' ORDER BY priority_rank",
        )
        .expect("prepare");
    let rows: Vec<(String, String, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("query")
        .filter_map(Result::ok)
        .collect();

    assert!(
        rows.len() >= 2,
        "expected at least 2 gaps, got {}",
        rows.len()
    );

    // First row should be High+never_tested (C1)
    assert_eq!(rows[0].0, "C1", "expected C1 first (High+never_tested)");
    assert_eq!(rows[0].1, "never_tested");

    // Second row should be Moderate+stale (C2)
    assert_eq!(rows[1].0, "C2", "expected C2 second (Moderate+stale)");
    assert_eq!(rows[1].1, "stale");

    // Verify priority ordering
    assert!(
        rows[0].2 < rows[1].2,
        "C1 priority_rank ({}) should be less than C2 ({})",
        rows[0].2,
        rows[1].2,
    );
}

#[test]
fn v_coverage_gaps_failing_control_appears() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw");
    store
        .upsert_control(&make_control("C1", "DORA", Severity::High))
        .expect("c1");
    store
        .upsert_org_entity(&project_entity("proj-1", "P1", None))
        .expect("entity");

    let now = Utc::now();
    let ev = make_evidence(
        "proj-1",
        "C1",
        EvidenceResult::Fail,
        now,
        EvidenceType::ChaosExperiment,
    );
    store.upsert_evidence(&ev).expect("ev");

    let gap_reason: String = store
        .conn()
        .query_row(
            "SELECT gap_reason FROM v_coverage_gaps \
             WHERE entity_id = 'proj-1' AND control_id = 'C1'",
            [],
            |row| row.get(0),
        )
        .expect("query");

    assert_eq!(gap_reason, "failing");
}

#[test]
fn v_coverage_gaps_partial_control_appears() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw");
    store
        .upsert_control(&make_control("C1", "DORA", Severity::High))
        .expect("c1");
    store
        .upsert_org_entity(&project_entity("proj-1", "P1", None))
        .expect("entity");

    let now = Utc::now();
    let ev = make_evidence(
        "proj-1",
        "C1",
        EvidenceResult::Partial,
        now,
        EvidenceType::ChaosExperiment,
    );
    store.upsert_evidence(&ev).expect("ev");

    let gap_reason: String = store
        .conn()
        .query_row(
            "SELECT gap_reason FROM v_coverage_gaps \
             WHERE entity_id = 'proj-1' AND control_id = 'C1'",
            [],
            |row| row.get(0),
        )
        .expect("query");

    assert_eq!(gap_reason, "partial");
}

// ---------------------------------------------------------------------------
// T4.3: v_cross_framework_map
// ---------------------------------------------------------------------------

#[test]
fn v_cross_framework_map_bidirectional_mapping_appears() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw1");
    store.upsert_framework(&nist_fw()).expect("fw2");

    store
        .upsert_control(&make_control("D1", "DORA", Severity::High))
        .expect("d1");
    store
        .upsert_control(&make_control("N1", "NIST", Severity::High))
        .expect("n1");

    let mapping = ControlMapping::new(
        ControlId::new("D1").expect("valid"),
        ControlId::new("N1").expect("valid"),
        MappingRelationship::Equivalent,
        Confidence::High,
        MappingDirection::Bidirectional,
        MappingProvenance::NistOlir,
    );
    store.upsert_mapping(&mapping).expect("mapping");

    let (ctrl_a, fw_a, ctrl_b, fw_b): (String, String, String, String) = store
        .conn()
        .query_row(
            "SELECT control_a, framework_a, control_b, framework_b \
             FROM v_cross_framework_map LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("query");

    assert_eq!(ctrl_a, "D1");
    assert_eq!(fw_a, "DORA");
    assert_eq!(ctrl_b, "N1");
    assert_eq!(fw_b, "NIST");
}

#[test]
fn v_cross_framework_map_shows_relationship_and_confidence() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw1");
    store.upsert_framework(&nist_fw()).expect("fw2");

    store
        .upsert_control(&make_control("D1", "DORA", Severity::High))
        .expect("d1");
    store
        .upsert_control(&make_control("N1", "NIST", Severity::High))
        .expect("n1");

    let mapping = ControlMapping::new(
        ControlId::new("D1").expect("valid"),
        ControlId::new("N1").expect("valid"),
        MappingRelationship::Partial,
        Confidence::Medium,
        MappingDirection::SourceToTarget,
        MappingProvenance::Manual,
    );
    store.upsert_mapping(&mapping).expect("mapping");

    let (relationship, confidence): (String, String) = store
        .conn()
        .query_row(
            "SELECT relationship, confidence FROM v_cross_framework_map LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query");

    assert_eq!(relationship, "Partial");
    assert_eq!(confidence, "Medium");
}

#[test]
fn v_cross_framework_map_multiple_mappings() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw1");
    store.upsert_framework(&nist_fw()).expect("fw2");

    for id in ["D1", "D2"] {
        store
            .upsert_control(&make_control(id, "DORA", Severity::High))
            .expect("ctrl");
    }
    store
        .upsert_control(&make_control("N1", "NIST", Severity::High))
        .expect("n1");

    // D1 -> N1 and D2 -> N1
    for src in ["D1", "D2"] {
        let mapping = ControlMapping::new(
            ControlId::new(src).expect("valid"),
            ControlId::new("N1").expect("valid"),
            MappingRelationship::Equivalent,
            Confidence::High,
            MappingDirection::Bidirectional,
            MappingProvenance::NistOlir,
        );
        store.upsert_mapping(&mapping).expect("mapping");
    }

    let count: usize = store
        .conn()
        .query_row("SELECT COUNT(*) FROM v_cross_framework_map", [], |row| {
            row.get(0)
        })
        .expect("query");

    assert_eq!(count, 2);
}

// ---------------------------------------------------------------------------
// T4.3: v_evidence_freshness
// ---------------------------------------------------------------------------

#[test]
fn v_evidence_freshness_counts_fresh_and_stale() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw");
    store
        .upsert_control(&make_control("C1", "DORA", Severity::High))
        .expect("c1");
    store
        .upsert_org_entity(&project_entity("proj-1", "P1", None))
        .expect("entity");

    let now = Utc::now();
    let et = EvidenceType::ChaosExperiment;

    // Fresh evidence
    let fresh_ev = make_evidence("proj-1", "C1", EvidenceResult::Pass, now, et);
    store.upsert_evidence(&fresh_ev).expect("ev1");

    // Stale evidence (different evidence_id)
    let old = now - Duration::days(200);
    let stale_ev = Evidence {
        evidence_id: EvidenceId::new(),
        entity_id: EntityId::new("proj-1"),
        control_id: ControlId::new("C1").expect("valid"),
        evidence_type: et,
        source_system: SourceSystem::new("tumult"),
        result: EvidenceResult::Pass,
        score: None,
        metadata: serde_json::Value::Null,
        observed_at: old,
        expires_at: compute_expires_at(old, &et),
    };
    store.upsert_evidence(&stale_ev).expect("ev2");

    let (total, fresh, stale): (i64, i64, i64) = store
        .conn()
        .query_row(
            "SELECT total_evidence, fresh, stale FROM v_evidence_freshness \
             WHERE entity_id = 'proj-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query");

    assert_eq!(total, 2, "total should be 2");
    assert_eq!(fresh, 1, "fresh should be 1");
    assert_eq!(stale, 1, "stale should be 1");
}

#[test]
fn v_evidence_freshness_pct_correct() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw");
    store
        .upsert_control(&make_control("C1", "DORA", Severity::High))
        .expect("c1");
    store
        .upsert_org_entity(&project_entity("proj-1", "P1", None))
        .expect("entity");

    let now = Utc::now();
    let et = EvidenceType::ChaosExperiment;

    // 2 fresh, 2 stale => 50% freshness
    for _ in 0..2 {
        let ev = make_evidence("proj-1", "C1", EvidenceResult::Pass, now, et);
        store.upsert_evidence(&ev).expect("ev");
    }
    let old = now - Duration::days(200);
    for _ in 0..2 {
        let ev = Evidence {
            evidence_id: EvidenceId::new(),
            entity_id: EntityId::new("proj-1"),
            control_id: ControlId::new("C1").expect("valid"),
            evidence_type: et,
            source_system: SourceSystem::new("tumult"),
            result: EvidenceResult::Pass,
            score: None,
            metadata: serde_json::Value::Null,
            observed_at: old,
            expires_at: compute_expires_at(old, &et),
        };
        store.upsert_evidence(&ev).expect("ev");
    }

    let freshness_pct: f64 = store
        .conn()
        .query_row(
            "SELECT freshness_pct FROM v_evidence_freshness WHERE entity_id = 'proj-1'",
            [],
            |row| row.get(0),
        )
        .expect("query");

    assert!(
        (freshness_pct - 50.0).abs() < f64::EPSILON,
        "expected 50% freshness, got {freshness_pct}"
    );
}

#[test]
fn v_evidence_freshness_groups_by_type_and_source() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw");
    store
        .upsert_control(&make_control("C1", "DORA", Severity::High))
        .expect("c1");
    store
        .upsert_org_entity(&project_entity("proj-1", "P1", None))
        .expect("entity");

    let now = Utc::now();

    // ChaosExperiment evidence
    let ev1 = make_evidence(
        "proj-1",
        "C1",
        EvidenceResult::Pass,
        now,
        EvidenceType::ChaosExperiment,
    );
    store.upsert_evidence(&ev1).expect("ev1");

    // GameDay evidence
    let ev2 = Evidence {
        evidence_id: EvidenceId::new(),
        entity_id: EntityId::new("proj-1"),
        control_id: ControlId::new("C1").expect("valid"),
        evidence_type: EvidenceType::GameDay,
        source_system: SourceSystem::new("tumult"),
        result: EvidenceResult::Pass,
        score: None,
        metadata: serde_json::Value::Null,
        observed_at: now,
        expires_at: compute_expires_at(now, &EvidenceType::GameDay),
    };
    store.upsert_evidence(&ev2).expect("ev2");

    let row_count: usize = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM v_evidence_freshness WHERE entity_id = 'proj-1'",
            [],
            |row| row.get(0),
        )
        .expect("query");

    // Should have 2 rows: one per evidence_type
    assert_eq!(row_count, 2, "expected 2 freshness rows (one per type)");
}

// ---------------------------------------------------------------------------
// T4.3: v_scores covered count cross-validation
// ---------------------------------------------------------------------------

#[test]
fn v_scores_covered_includes_partial() {
    let store = setup_store();
    store.upsert_framework(&dora_fw()).expect("fw");

    let controls = vec![
        make_control("C1", "DORA", Severity::High),
        make_control("C2", "DORA", Severity::High),
    ];
    for c in &controls {
        store.upsert_control(c).expect("ctrl");
    }
    store
        .upsert_org_entity(&project_entity("proj-1", "P1", None))
        .expect("entity");

    let now = Utc::now();
    let et = EvidenceType::ChaosExperiment;
    let evidence = vec![
        make_evidence("proj-1", "C1", EvidenceResult::Pass, now, et),
        make_evidence("proj-1", "C2", EvidenceResult::Partial, now, et),
    ];
    for ev in &evidence {
        store.upsert_evidence(ev).expect("ev");
    }

    let view_covered: i64 = store
        .conn()
        .query_row(
            "SELECT controls_covered FROM v_scores \
             WHERE entity_id = 'proj-1' AND framework_id = 'DORA'",
            [],
            |row| row.get(0),
        )
        .expect("query");

    let engine = compute_entity_framework_score(
        &EntityId::new("proj-1"),
        &FrameworkId::new("DORA").expect("valid"),
        &controls,
        &evidence,
        now,
    );

    #[allow(clippy::cast_possible_wrap)]
    {
        assert_eq!(
            engine.controls_covered as i64, view_covered,
            "covered mismatch: engine={}, view={view_covered}",
            engine.controls_covered,
        );
    }
}
