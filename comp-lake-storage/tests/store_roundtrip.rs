//! Store round-trip tests: schema creation, CRUD, upsert semantics,
//! Arrow schema validation, Parquet export/import, and `DuckDB` file export.

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
use comp_lake_storage::arrow_schema;
use comp_lake_storage::export::{export_duckdb_file, export_parquet, import_parquet};
use comp_lake_storage::schema;
use comp_lake_storage::store::CompLakeStore;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn dora_fw() -> Framework {
    Framework::builder(FrameworkId::new("DORA").expect("valid"), "DORA")
        .version("2022/2554")
        .region(Region::Eu)
        .authority("EU/EP")
        .harvest_source(HarvestSource::CellarSparql)
        .build()
}

fn make_control(id: &str, fw: &str) -> Control {
    Control::builder(
        ControlId::new(id).expect("valid"),
        FrameworkId::new(fw).expect("valid"),
        format!("Control {id}"),
    )
    .severity(Severity::High)
    .family(ControlFamily::new("Testing"))
    .testing_relevant(true)
    .build()
}

fn make_entity(id: &str, etype: EntityType, parent: Option<&str>) -> OrgEntity {
    OrgEntity::new(
        EntityId::new(id),
        etype,
        format!("Entity {id}"),
        parent.map(EntityId::new),
    )
}

fn make_evidence(entity: &str, ctrl_id: &str, result: EvidenceResult) -> Evidence {
    let now = Utc::now();
    let et = EvidenceType::ChaosExperiment;
    Evidence {
        evidence_id: EvidenceId::new(),
        entity_id: EntityId::new(entity),
        control_id: ControlId::new(ctrl_id).expect("valid"),
        evidence_type: et,
        source_system: SourceSystem::new("tumult"),
        result,
        score: Some(result.as_score()),
        metadata: serde_json::json!({"test": true}),
        observed_at: now,
        expires_at: compute_expires_at(now, &et),
    }
}

// ---------------------------------------------------------------------------
// Schema creation tests
// ---------------------------------------------------------------------------

#[test]
fn schema_creates_on_empty_in_memory_db() {
    let conn = duckdb::Connection::open_in_memory().expect("open");
    schema::create_schema(&conn).expect("create_schema");

    let version = schema::schema_version(&conn).expect("version");
    assert_eq!(version.as_deref(), Some("1"));
}

#[test]
fn schema_creates_all_expected_tables() {
    let conn = duckdb::Connection::open_in_memory().expect("open");
    schema::create_schema(&conn).expect("create_schema");

    let expected_tables = [
        "frameworks",
        "controls",
        "control_mappings",
        "org_hierarchy",
        "evidence",
        "harvest_log",
        "schema_meta",
    ];
    for table in &expected_tables {
        let count: usize = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap_or_else(|e| panic!("table {table} should exist: {e}"));
        if *table == "schema_meta" {
            // schema_meta gets a version row during create_schema
            assert_eq!(count, 1, "schema_meta should have version row");
        } else {
            assert_eq!(count, 0, "table {table} should be empty initially");
        }
    }
}

#[test]
fn schema_migrate_idempotent() {
    let conn = duckdb::Connection::open_in_memory().expect("open");
    schema::migrate(&conn).expect("migrate 1");
    schema::migrate(&conn).expect("migrate 2");
    let version = schema::schema_version(&conn).expect("version");
    assert_eq!(version.as_deref(), Some("1"));
}

// ---------------------------------------------------------------------------
// Framework round-trip
// ---------------------------------------------------------------------------

#[test]
fn framework_insert_and_read() {
    let store = CompLakeStore::in_memory().expect("store");
    let fw = dora_fw();
    store.upsert_framework(&fw).expect("insert");

    let (name, version): (String, String) = store
        .conn()
        .query_row(
            "SELECT name, version FROM frameworks WHERE framework_id = 'DORA'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read");

    assert_eq!(name, "DORA");
    assert_eq!(version, "2022/2554");
}

#[test]
fn framework_upsert_updates_existing() {
    let store = CompLakeStore::in_memory().expect("store");
    store.upsert_framework(&dora_fw()).expect("insert 1");

    // Update version
    let updated = Framework::builder(FrameworkId::new("DORA").expect("valid"), "DORA v2")
        .version("2024/001")
        .region(Region::Eu)
        .authority("EU/EP")
        .harvest_source(HarvestSource::CellarSparql)
        .build();
    store.upsert_framework(&updated).expect("insert 2");

    assert_eq!(store.count("frameworks").expect("count"), 1);

    let name: String = store
        .conn()
        .query_row(
            "SELECT name FROM frameworks WHERE framework_id = 'DORA'",
            [],
            |row| row.get(0),
        )
        .expect("read");
    assert_eq!(name, "DORA v2");
}

// ---------------------------------------------------------------------------
// Control round-trip
// ---------------------------------------------------------------------------

#[test]
fn control_insert_and_read() {
    let store = CompLakeStore::in_memory().expect("store");
    store.upsert_framework(&dora_fw()).expect("fw");

    let ctrl = make_control("C1", "DORA");
    store.upsert_control(&ctrl).expect("insert");

    let (title, severity, testing): (String, String, bool) = store
        .conn()
        .query_row(
            "SELECT title, severity, testing_relevant FROM controls WHERE control_id = 'C1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read");

    assert_eq!(title, "Control C1");
    assert_eq!(severity, "High");
    assert!(testing);
}

#[test]
fn control_upsert_updates_existing() {
    let store = CompLakeStore::in_memory().expect("store");
    store.upsert_framework(&dora_fw()).expect("fw");
    store
        .upsert_control(&make_control("C1", "DORA"))
        .expect("insert 1");

    let updated = Control::builder(
        ControlId::new("C1").expect("valid"),
        FrameworkId::new("DORA").expect("valid"),
        "Updated Title",
    )
    .severity(Severity::Low)
    .testing_relevant(false)
    .build();
    store.upsert_control(&updated).expect("insert 2");

    assert_eq!(store.count("controls").expect("count"), 1);

    let (title, severity): (String, String) = store
        .conn()
        .query_row(
            "SELECT title, severity FROM controls WHERE control_id = 'C1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read");
    assert_eq!(title, "Updated Title");
    assert_eq!(severity, "Low");
}

// ---------------------------------------------------------------------------
// Evidence round-trip
// ---------------------------------------------------------------------------

#[test]
fn evidence_insert_and_read() {
    let store = CompLakeStore::in_memory().expect("store");
    store.upsert_framework(&dora_fw()).expect("fw");
    store
        .upsert_control(&make_control("C1", "DORA"))
        .expect("ctrl");
    store
        .upsert_org_entity(&make_entity("proj-1", EntityType::Project, None))
        .expect("entity");

    let ev = make_evidence("proj-1", "C1", EvidenceResult::Pass);
    store.upsert_evidence(&ev).expect("insert");

    let (result_str, score_val): (String, f64) = store
        .conn()
        .query_row(
            "SELECT result, score FROM evidence WHERE entity_id = 'proj-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read");

    assert_eq!(result_str, "Pass");
    assert!((score_val - 1.0).abs() < f64::EPSILON);
}

#[test]
fn evidence_upsert_updates_existing() {
    let store = CompLakeStore::in_memory().expect("store");
    store.upsert_framework(&dora_fw()).expect("fw");
    store
        .upsert_control(&make_control("C1", "DORA"))
        .expect("ctrl");
    store
        .upsert_org_entity(&make_entity("proj-1", EntityType::Project, None))
        .expect("entity");

    let now = Utc::now();
    let et = EvidenceType::ChaosExperiment;
    let ev_id = EvidenceId::new();
    let ev = Evidence {
        evidence_id: ev_id.clone(),
        entity_id: EntityId::new("proj-1"),
        control_id: ControlId::new("C1").expect("valid"),
        evidence_type: et,
        source_system: SourceSystem::new("tumult"),
        result: EvidenceResult::Fail,
        score: Some(0.0),
        metadata: serde_json::json!({}),
        observed_at: now,
        expires_at: compute_expires_at(now, &et),
    };
    store.upsert_evidence(&ev).expect("insert 1");

    // Update same evidence_id with Pass result
    let ev2 = Evidence {
        evidence_id: ev_id,
        entity_id: EntityId::new("proj-1"),
        control_id: ControlId::new("C1").expect("valid"),
        evidence_type: et,
        source_system: SourceSystem::new("tumult"),
        result: EvidenceResult::Pass,
        score: Some(1.0),
        metadata: serde_json::json!({"updated": true}),
        observed_at: now,
        expires_at: compute_expires_at(now, &et),
    };
    store.upsert_evidence(&ev2).expect("insert 2");

    assert_eq!(store.count("evidence").expect("count"), 1);

    let result_str: String = store
        .conn()
        .query_row(
            "SELECT result FROM evidence WHERE entity_id = 'proj-1'",
            [],
            |row| row.get(0),
        )
        .expect("read");
    assert_eq!(result_str, "Pass");
}

// ---------------------------------------------------------------------------
// Org entity round-trip
// ---------------------------------------------------------------------------

#[test]
fn org_entity_insert_and_read() {
    let store = CompLakeStore::in_memory().expect("store");
    let entity = make_entity("team-a", EntityType::Team, None);
    store.upsert_org_entity(&entity).expect("insert");

    let (name, etype): (String, String) = store
        .conn()
        .query_row(
            "SELECT name, entity_type FROM org_hierarchy WHERE entity_id = 'team-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read");

    assert_eq!(name, "Entity team-a");
    assert_eq!(etype, "Team");
}

#[test]
fn org_entity_upsert_updates_name() {
    let store = CompLakeStore::in_memory().expect("store");
    store
        .upsert_org_entity(&make_entity("team-a", EntityType::Team, None))
        .expect("insert 1");

    let updated = OrgEntity::new(
        EntityId::new("team-a"),
        EntityType::Team,
        "Renamed Team",
        None,
    );
    store.upsert_org_entity(&updated).expect("insert 2");

    assert_eq!(store.count("org_hierarchy").expect("count"), 1);

    let name: String = store
        .conn()
        .query_row(
            "SELECT name FROM org_hierarchy WHERE entity_id = 'team-a'",
            [],
            |row| row.get(0),
        )
        .expect("read");
    assert_eq!(name, "Renamed Team");
}

// ---------------------------------------------------------------------------
// Control mappings round-trip
// ---------------------------------------------------------------------------

#[test]
fn mapping_insert_and_read() {
    let store = CompLakeStore::in_memory().expect("store");
    store.upsert_framework(&dora_fw()).expect("fw1");
    let nist = Framework::builder(FrameworkId::new("NIST").expect("valid"), "NIST")
        .version("r5")
        .authority("NIST")
        .build();
    store.upsert_framework(&nist).expect("fw2");
    store
        .upsert_control(&make_control("D1", "DORA"))
        .expect("d1");
    store
        .upsert_control(&make_control("N1", "NIST"))
        .expect("n1");

    let mapping = ControlMapping::new(
        ControlId::new("D1").expect("valid"),
        ControlId::new("N1").expect("valid"),
        MappingRelationship::Equivalent,
        Confidence::High,
        MappingDirection::Bidirectional,
        MappingProvenance::NistOlir,
    );
    store.upsert_mapping(&mapping).expect("insert");

    let (rel, conf): (String, String) = store
        .conn()
        .query_row(
            "SELECT relationship, confidence FROM control_mappings \
             WHERE source_control = 'D1' AND target_control = 'N1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read");

    assert_eq!(rel, "Equivalent");
    assert_eq!(conf, "High");
}

#[test]
fn mapping_upsert_updates_existing() {
    let store = CompLakeStore::in_memory().expect("store");
    store.upsert_framework(&dora_fw()).expect("fw1");
    let nist = Framework::builder(FrameworkId::new("NIST").expect("valid"), "NIST")
        .version("r5")
        .authority("NIST")
        .build();
    store.upsert_framework(&nist).expect("fw2");
    store
        .upsert_control(&make_control("D1", "DORA"))
        .expect("d1");
    store
        .upsert_control(&make_control("N1", "NIST"))
        .expect("n1");

    let mapping1 = ControlMapping::new(
        ControlId::new("D1").expect("valid"),
        ControlId::new("N1").expect("valid"),
        MappingRelationship::Equivalent,
        Confidence::Low,
        MappingDirection::Bidirectional,
        MappingProvenance::Manual,
    );
    store.upsert_mapping(&mapping1).expect("insert 1");

    let mapping2 = ControlMapping::new(
        ControlId::new("D1").expect("valid"),
        ControlId::new("N1").expect("valid"),
        MappingRelationship::Partial,
        Confidence::High,
        MappingDirection::SourceToTarget,
        MappingProvenance::NistOlir,
    );
    store.upsert_mapping(&mapping2).expect("insert 2");

    assert_eq!(store.count("control_mappings").expect("count"), 1);

    let conf: String = store
        .conn()
        .query_row(
            "SELECT confidence FROM control_mappings \
             WHERE source_control = 'D1' AND target_control = 'N1'",
            [],
            |row| row.get(0),
        )
        .expect("read");
    assert_eq!(conf, "High");
}

// ---------------------------------------------------------------------------
// Arrow schema matches DuckDB table columns
// ---------------------------------------------------------------------------

#[test]
fn arrow_frameworks_schema_field_count_matches_duckdb() {
    let store = CompLakeStore::in_memory().expect("store");
    let arrow = arrow_schema::frameworks_schema();

    let mut stmt = store
        .conn()
        .prepare("SELECT column_name FROM information_schema.columns WHERE table_name = 'frameworks' ORDER BY ordinal_position")
        .expect("prepare");
    let duck_cols: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .expect("query")
        .filter_map(Result::ok)
        .collect();

    // Arrow schema excludes created_at/updated_at (auto-managed by DuckDB)
    let arrow_fields: Vec<&str> = arrow.fields().iter().map(|f| f.name().as_str()).collect();

    // Verify each Arrow field exists in DuckDB (Arrow -> DuckDB)
    for field in &arrow_fields {
        assert!(
            duck_cols.contains(&field.to_string()),
            "Arrow field '{field}' not found in DuckDB frameworks table. DuckDB cols: {duck_cols:?}"
        );
    }

    // Verify reverse: every DuckDB column (except auto-managed) has an Arrow field (DuckDB -> Arrow).
    let auto_managed = ["created_at", "updated_at"];
    let arrow_names: std::collections::HashSet<&str> = arrow_fields.iter().copied().collect();
    for col in &duck_cols {
        if auto_managed.contains(&col.as_str()) {
            continue;
        }
        assert!(
            arrow_names.contains(col.as_str()),
            "DuckDB frameworks column '{col}' has no Arrow field — update arrow_schema.rs"
        );
    }
}

#[test]
fn arrow_controls_schema_field_count_matches_duckdb() {
    let store = CompLakeStore::in_memory().expect("store");
    let arrow = arrow_schema::controls_schema();

    let mut stmt = store
        .conn()
        .prepare("SELECT column_name FROM information_schema.columns WHERE table_name = 'controls' ORDER BY ordinal_position")
        .expect("prepare");
    let duck_cols: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .expect("query")
        .filter_map(Result::ok)
        .collect();

    let arrow_fields: Vec<&str> = arrow.fields().iter().map(|f| f.name().as_str()).collect();

    for field in &arrow_fields {
        assert!(
            duck_cols.contains(&field.to_string()),
            "Arrow field '{field}' not found in DuckDB controls table. DuckDB cols: {duck_cols:?}"
        );
    }
}

#[test]
fn arrow_evidence_schema_field_count_matches_duckdb() {
    let store = CompLakeStore::in_memory().expect("store");
    let arrow = arrow_schema::evidence_schema();

    let mut stmt = store
        .conn()
        .prepare("SELECT column_name FROM information_schema.columns WHERE table_name = 'evidence' ORDER BY ordinal_position")
        .expect("prepare");
    let duck_cols: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .expect("query")
        .filter_map(Result::ok)
        .collect();

    let arrow_fields: Vec<&str> = arrow.fields().iter().map(|f| f.name().as_str()).collect();

    // Verify each Arrow field exists in DuckDB (Arrow -> DuckDB)
    for field in &arrow_fields {
        assert!(
            duck_cols.contains(&field.to_string()),
            "Arrow field '{field}' not found in DuckDB evidence table. DuckDB cols: {duck_cols:?}"
        );
    }

    // Verify reverse: every DuckDB column (except auto-managed) has an Arrow field (DuckDB -> Arrow).
    let auto_managed = ["created_at", "updated_at"];
    let arrow_names: std::collections::HashSet<&str> = arrow_fields.iter().copied().collect();
    for col in &duck_cols {
        if auto_managed.contains(&col.as_str()) {
            continue;
        }
        assert!(
            arrow_names.contains(col.as_str()),
            "DuckDB evidence column '{col}' has no Arrow field — update arrow_schema.rs"
        );
    }
}

// ---------------------------------------------------------------------------
// Parquet export / import round-trip
// ---------------------------------------------------------------------------

#[test]
fn parquet_frameworks_roundtrip_identical() {
    let fw = dora_fw();
    let batch = arrow_schema::frameworks_to_record_batch(&[fw]).expect("batch");

    let dir = tempfile::tempdir().expect("tmpdir");
    let path = dir.path().join("frameworks.parquet");

    export_parquet(&batch, &path).expect("export");
    let (schema, batches) = import_parquet(&path).expect("import");

    assert_eq!(schema.fields().len(), batch.schema().fields().len());
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].num_rows(), batch.num_rows());
    assert_eq!(batches[0].num_columns(), batch.num_columns());

    // Compare column values
    for col_idx in 0..batch.num_columns() {
        let original = batch.column(col_idx);
        let imported = batches[0].column(col_idx);
        assert_eq!(
            original.as_ref(),
            imported.as_ref(),
            "column {col_idx} mismatch after parquet round-trip"
        );
    }
}

#[test]
fn parquet_evidence_roundtrip_identical() {
    let now = Utc::now();
    let et = EvidenceType::ChaosExperiment;
    let ev = Evidence {
        evidence_id: EvidenceId::new(),
        entity_id: EntityId::new("proj-1"),
        control_id: ControlId::new("C1").expect("valid"),
        evidence_type: et,
        source_system: SourceSystem::new("tumult"),
        result: EvidenceResult::Pass,
        score: Some(1.0),
        metadata: serde_json::json!({"key": "value"}),
        observed_at: now,
        expires_at: compute_expires_at(now, &et),
    };
    let batch = arrow_schema::evidence_to_record_batch(&[ev]).expect("batch");

    let dir = tempfile::tempdir().expect("tmpdir");
    let path = dir.path().join("evidence.parquet");

    export_parquet(&batch, &path).expect("export");
    let (_, batches) = import_parquet(&path).expect("import");

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].num_rows(), 1);

    for col_idx in 0..batch.num_columns() {
        let original = batch.column(col_idx);
        let imported = batches[0].column(col_idx);
        assert_eq!(
            original.as_ref(),
            imported.as_ref(),
            "evidence column {col_idx} mismatch"
        );
    }
}

#[test]
fn parquet_scores_roundtrip_identical() {
    use comp_lake_core::scoring::ComplianceScore;

    let scores = vec![
        ComplianceScore::new(FrameworkId::new("DORA").expect("valid"), 10, 8, 7, 1),
        ComplianceScore::new(FrameworkId::new("NIST").expect("valid"), 20, 15, 12, 3),
    ];
    let batch = arrow_schema::scores_to_record_batch(&scores).expect("batch");

    let dir = tempfile::tempdir().expect("tmpdir");
    let path = dir.path().join("scores.parquet");

    export_parquet(&batch, &path).expect("export");
    let (_, batches) = import_parquet(&path).expect("import");

    let total_rows: usize = batches
        .iter()
        .map(arrow::array::RecordBatch::num_rows)
        .sum();
    assert_eq!(total_rows, 2);
}

// ---------------------------------------------------------------------------
// DuckDB file export is self-contained and queryable
// ---------------------------------------------------------------------------

#[test]
fn duckdb_export_self_contained_and_queryable() {
    let store = CompLakeStore::in_memory().expect("store");
    store.upsert_framework(&dora_fw()).expect("fw");
    store
        .upsert_control(&make_control("C1", "DORA"))
        .expect("ctrl");
    store
        .upsert_org_entity(&make_entity("proj-1", EntityType::Project, None))
        .expect("entity");
    let ev = make_evidence("proj-1", "C1", EvidenceResult::Pass);
    store.upsert_evidence(&ev).expect("ev");

    let dir = tempfile::tempdir().expect("tmpdir");
    let export_path = dir.path().join("standalone.duckdb");

    export_duckdb_file(store.conn(), &export_path).expect("export");

    // Reopen the exported file independently
    let export_conn = duckdb::Connection::open(&export_path).expect("reopen");

    // Query each table
    let fw_count: usize = export_conn
        .query_row("SELECT COUNT(*) FROM frameworks", [], |row| row.get(0))
        .expect("fw count");
    assert_eq!(fw_count, 1);

    let ctrl_count: usize = export_conn
        .query_row("SELECT COUNT(*) FROM controls", [], |row| row.get(0))
        .expect("ctrl count");
    assert_eq!(ctrl_count, 1);

    let ev_count: usize = export_conn
        .query_row("SELECT COUNT(*) FROM evidence", [], |row| row.get(0))
        .expect("ev count");
    assert_eq!(ev_count, 1);

    let org_count: usize = export_conn
        .query_row("SELECT COUNT(*) FROM org_hierarchy", [], |row| row.get(0))
        .expect("org count");
    assert_eq!(org_count, 1);

    // Verify actual data content
    let fw_name: String = export_conn
        .query_row(
            "SELECT name FROM frameworks WHERE framework_id = 'DORA'",
            [],
            |row| row.get(0),
        )
        .expect("fw name");
    assert_eq!(fw_name, "DORA");
}

#[test]
fn duckdb_export_preserves_schema_meta() {
    let store = CompLakeStore::in_memory().expect("store");

    let dir = tempfile::tempdir().expect("tmpdir");
    let export_path = dir.path().join("meta.duckdb");

    export_duckdb_file(store.conn(), &export_path).expect("export");

    let export_conn = duckdb::Connection::open(&export_path).expect("reopen");
    let version: String = export_conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .expect("version");
    assert_eq!(version, "1");
}

// ---------------------------------------------------------------------------
// Count allowlist enforcement
// ---------------------------------------------------------------------------

#[test]
fn count_rejects_unknown_table() {
    let store = CompLakeStore::in_memory().expect("store");
    let result = store.count("nonexistent_table");
    assert!(result.is_err(), "count should reject unknown table names");
}

#[test]
fn count_accepts_all_known_tables() {
    let store = CompLakeStore::in_memory().expect("store");
    let known = [
        "frameworks",
        "controls",
        "control_mappings",
        "org_hierarchy",
        "evidence",
        "harvest_log",
        "schema_meta",
    ];
    for table in &known {
        let count = store.count(table);
        assert!(count.is_ok(), "count should accept known table: {table}");
    }
}

// ---------------------------------------------------------------------------
// File-backed store persistence
// ---------------------------------------------------------------------------

#[test]
fn file_backed_store_persists_across_opens() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let path = dir.path().join("persist.duckdb");

    // First open: write data
    {
        let store = CompLakeStore::open(&path).expect("open 1");
        store.upsert_framework(&dora_fw()).expect("fw");
        store
            .upsert_control(&make_control("C1", "DORA"))
            .expect("ctrl");
        store
            .upsert_org_entity(&make_entity("proj-1", EntityType::Project, None))
            .expect("entity");
    }

    // Second open: verify data persists
    {
        let store = CompLakeStore::open(&path).expect("open 2");
        assert_eq!(store.count("frameworks").expect("count"), 1);
        assert_eq!(store.count("controls").expect("count"), 1);
        assert_eq!(store.count("org_hierarchy").expect("count"), 1);
    }
}
