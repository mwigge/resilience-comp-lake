use chrono::Utc;
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

/// End-to-end: seed → store → views → scoring engine cross-validation
#[test]
fn seed_store_score_roundtrip() {
    let store = CompLakeStore::in_memory().expect("create in-memory store");
    views::create_views(store.conn()).expect("create views");

    // Seed framework
    let fw = Framework::builder(
        FrameworkId::new("DORA").expect("valid framework ID"),
        "DORA",
    )
    .version("2022/2554")
    .region(Region::Eu)
    .authority("EU/EP")
    .harvest_source(HarvestSource::Manual)
    .build();
    store.upsert_framework(&fw).expect("upsert framework");

    // Seed 4 controls (all testing-relevant)
    let ctrl_ids = ["C1", "C2", "C3", "C4"];
    let mut controls = Vec::new();
    for id in &ctrl_ids {
        let ctrl = Control::builder(
            ControlId::new(*id).expect("valid control ID"),
            FrameworkId::new("DORA").expect("valid framework ID"),
            format!("Control {id}"),
        )
        .severity(Severity::High)
        .family(ControlFamily::new("Testing"))
        .testing_relevant(true)
        .build();
        store.upsert_control(&ctrl).expect("upsert control");
        controls.push(ctrl);
    }

    // Seed org entity
    let entity = OrgEntity::new(EntityId::new("team-a"), EntityType::Project, "Team A", None);
    store.upsert_org_entity(&entity).expect("upsert org entity");

    // Seed evidence: C1=Pass, C2=Pass, C3=Fail, C4=no evidence
    let now = Utc::now();
    let et = EvidenceType::ChaosExperiment;
    for (ctrl_id, result) in [
        ("C1", EvidenceResult::Pass),
        ("C2", EvidenceResult::Pass),
        ("C3", EvidenceResult::Fail),
    ] {
        let ev = Evidence {
            evidence_id: EvidenceId::new(),
            entity_id: EntityId::new("team-a"),
            control_id: ControlId::new(ctrl_id).expect("valid control ID"),
            evidence_type: et,
            source_system: SourceSystem::new("tumult"),
            result,
            score: None,
            metadata: serde_json::json!({}),
            observed_at: now,
            expires_at: compute_expires_at(now, &et),
        };
        store.upsert_evidence(&ev).expect("upsert evidence");
    }

    // Verify via DuckDB view
    let (view_passing, view_total, view_score): (i64, i64, f64) = store
        .conn()
        .query_row(
            "SELECT controls_passing, controls_total, score FROM v_scores \
             WHERE entity_id = ? AND framework_id = ?",
            ["team-a", "DORA"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query v_scores");
    assert_eq!(view_total, 4);
    assert_eq!(view_passing, 2);
    assert!((view_score - 50.0).abs() < f64::EPSILON);

    // Cross-validate with Rust scoring engine
    let evidence: Vec<Evidence> = vec![
        make_ev("C1", EvidenceResult::Pass, now, et),
        make_ev("C2", EvidenceResult::Pass, now, et),
        make_ev("C3", EvidenceResult::Fail, now, et),
    ];
    let engine_score = compute_entity_framework_score(
        &EntityId::new("team-a"),
        &FrameworkId::new("DORA").expect("valid framework ID"),
        &controls,
        &evidence,
        now,
    );
    assert!((engine_score.score - view_score).abs() < f64::EPSILON);
    #[allow(clippy::cast_possible_wrap)]
    {
        assert_eq!(engine_score.controls_total as i64, view_total);
        assert_eq!(engine_score.controls_passing as i64, view_passing);
    }
}

/// End-to-end: cross-framework mapping stored and queryable via view
#[test]
fn cross_framework_mapping_view() {
    let store = CompLakeStore::in_memory().expect("create in-memory store");
    views::create_views(store.conn()).expect("create views");

    // Two frameworks
    for (id, name) in [("DORA", "DORA"), ("ISO", "ISO 27001")] {
        let fw = Framework::builder(FrameworkId::new(id).expect("valid framework ID"), name)
            .version("1.0")
            .authority("Test")
            .build();
        store.upsert_framework(&fw).expect("upsert framework");
    }

    // One control each
    for (id, fw) in [("D1", "DORA"), ("I1", "ISO")] {
        let ctrl = Control::builder(
            ControlId::new(id).expect("valid control ID"),
            FrameworkId::new(fw).expect("valid framework ID"),
            format!("Control {id}"),
        )
        .severity(Severity::High)
        .testing_relevant(true)
        .build();
        store.upsert_control(&ctrl).expect("upsert control");
    }

    // Mapping
    let mapping = ControlMapping::new(
        ControlId::new("D1").expect("valid"),
        ControlId::new("I1").expect("valid"),
        MappingRelationship::Equivalent,
        Confidence::High,
        MappingDirection::Bidirectional,
        MappingProvenance::EbaMapping,
    );
    store.upsert_mapping(&mapping).expect("upsert mapping");

    // Verify via view
    let count: usize = store
        .conn()
        .query_row("SELECT COUNT(*) FROM v_cross_framework_map", [], |row| {
            row.get(0)
        })
        .expect("count mappings");
    assert_eq!(count, 1);
}

/// End-to-end: coverage gaps view shows untested controls
#[test]
fn coverage_gaps_identify_untested() {
    let store = CompLakeStore::in_memory().expect("create in-memory store");
    views::create_views(store.conn()).expect("create views");

    let fw = Framework::builder(
        FrameworkId::new("DORA").expect("valid framework ID"),
        "DORA",
    )
    .version("1.0")
    .authority("Test")
    .build();
    store.upsert_framework(&fw).expect("upsert framework");

    let ctrl = Control::builder(
        ControlId::new("C1").expect("valid"),
        FrameworkId::new("DORA").expect("valid framework ID"),
        "Untested Control",
    )
    .severity(Severity::High)
    .testing_relevant(true)
    .build();
    store.upsert_control(&ctrl).expect("upsert control");

    let entity = OrgEntity::new(EntityId::new("team-a"), EntityType::Project, "Team A", None);
    store.upsert_org_entity(&entity).expect("upsert org entity");

    // No evidence — should appear as gap
    let gap_reason: String = store
        .conn()
        .query_row(
            "SELECT gap_reason FROM v_coverage_gaps WHERE entity_id = ? AND control_id = ?",
            ["team-a", "C1"],
            |row| row.get(0),
        )
        .expect("query coverage gaps");
    assert_eq!(gap_reason, "never_tested");
}

fn make_ev(
    ctrl_id: &str,
    result: EvidenceResult,
    now: chrono::DateTime<Utc>,
    et: EvidenceType,
) -> Evidence {
    Evidence {
        evidence_id: EvidenceId::new(),
        entity_id: EntityId::new("team-a"),
        control_id: ControlId::new(ctrl_id).expect("valid control ID"),
        evidence_type: et,
        source_system: SourceSystem::new("tumult"),
        result,
        score: None,
        metadata: serde_json::json!({}),
        observed_at: now,
        expires_at: compute_expires_at(now, &et),
    }
}
