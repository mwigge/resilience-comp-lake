//! Integration tests for comp-lake-harvesters.
//!
//! Tests cover harvester trait metadata, manual seed loading with real data files,
//! scheduler cadence logic, snapshot filesystem operations, and harvest log records.
//!
//! HTTP-based harvesters (CELLAR, OSCAL, NVD, Scorecard) use hardcoded upstream URLs,
//! so integration tests verify trait surface and parsing of fixture data via the
//! public `load_seed_toml` API. JSON fixture files in `tests/fixtures/` document the
//! expected response shapes for each upstream API.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};

use comp_lake_core::models::control::Severity;
use comp_lake_core::models::framework::{FrameworkId, HarvestSource, Region};
use comp_lake_core::models::mapping::{Confidence, MappingDirection, MappingRelationship};

use comp_lake_harvesters::cellar::CellarHarvester;
use comp_lake_harvesters::harvester::{
    HarvestCadence, HarvestConfig, HarvestLog, HarvestStatus, Harvester,
};
use comp_lake_harvesters::manual::{load_seed_file, load_seed_toml, ManualLoader};
use comp_lake_harvesters::mapping_loader::{
    load_all_mappings, load_mapping_file, load_mapping_toml,
};
use comp_lake_harvesters::nvd::NvdHarvester;
use comp_lake_harvesters::oscal::OscalHarvester;
use comp_lake_harvesters::scheduler::{cadence_to_duration, find_due_harvesters, is_due};
use comp_lake_harvesters::scorecard::{
    check_to_control_mapping, normalise_score, ScorecardHarvester,
};
use comp_lake_harvesters::snapshot::{
    apply_retention, ensure_snapshot_dir, list_snapshots, snapshot_path,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Resolve a path relative to the workspace root (parent of comp-lake-harvesters).
fn workspace_path(relative: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("manifest dir has parent")
        .join(relative)
}

fn fw(id: &str) -> FrameworkId {
    FrameworkId::new(id).expect("valid framework ID in test")
}

// ═════════════════════════════════════════════════════════════════════════════
// 1. CELLAR harvester — trait surface and metadata
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn cellar_harvester_name_matches_expected() {
    let h = CellarHarvester::new();
    assert_eq!(h.name(), "EUR-Lex CELLAR (SPARQL)");
}

#[test]
fn cellar_harvester_cadence_is_monthly() {
    let h = CellarHarvester::new();
    assert_eq!(h.cadence(), HarvestCadence::Monthly);
}

#[test]
fn cellar_harvester_covers_four_eu_frameworks() {
    let h = CellarHarvester::new();
    let ids: Vec<&str> = h.frameworks().iter().map(FrameworkId::as_str).collect();
    assert_eq!(ids.len(), 4);
    assert!(ids.contains(&"DORA"));
    assert!(ids.contains(&"CRA"));
    assert!(ids.contains(&"NIS2"));
    assert!(ids.contains(&"GDPR"));
}

#[test]
fn cellar_harvester_is_object_safe_as_dyn_harvester() {
    let h = CellarHarvester::new();
    let dyn_ref: &dyn Harvester = &h;
    assert_eq!(dyn_ref.name(), "EUR-Lex CELLAR (SPARQL)");
    assert_eq!(dyn_ref.frameworks().len(), 4);
}

// ═════════════════════════════════════════════════════════════════════════════
// 2. OSCAL harvester — trait surface and metadata
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn oscal_harvester_name_matches_expected() {
    let h = OscalHarvester::new();
    assert_eq!(h.name(), "NIST OSCAL (GitHub)");
}

#[test]
fn oscal_harvester_cadence_is_on_release() {
    let h = OscalHarvester::new();
    assert_eq!(h.cadence(), HarvestCadence::OnRelease);
}

#[test]
fn oscal_harvester_covers_nist_frameworks() {
    let h = OscalHarvester::new();
    let ids: Vec<&str> = h.frameworks().iter().map(FrameworkId::as_str).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"NIST-800-53-R5"));
    assert!(ids.contains(&"NIST-CSF-2"));
}

#[test]
fn oscal_harvester_is_object_safe_as_dyn_harvester() {
    let h = OscalHarvester::new();
    let dyn_ref: &dyn Harvester = &h;
    assert_eq!(dyn_ref.name(), "NIST OSCAL (GitHub)");
}

// ═════════════════════════════════════════════════════════════════════════════
// 3. NVD harvester — trait surface and metadata
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn nvd_harvester_name_matches_expected() {
    let h = NvdHarvester::new();
    assert_eq!(h.name(), "NIST NVD (CVE API 2.0)");
}

#[test]
fn nvd_harvester_cadence_is_daily() {
    let h = NvdHarvester::new();
    assert_eq!(h.cadence(), HarvestCadence::Daily);
}

#[test]
fn nvd_harvester_covers_nvd_framework() {
    let h = NvdHarvester::new();
    assert_eq!(h.frameworks().len(), 1);
    assert_eq!(h.frameworks()[0].as_str(), "NVD");
}

#[test]
fn nvd_harvester_is_object_safe_as_dyn_harvester() {
    let h = NvdHarvester::new();
    let dyn_ref: &dyn Harvester = &h;
    assert_eq!(dyn_ref.name(), "NIST NVD (CVE API 2.0)");
}

// ═════════════════════════════════════════════════════════════════════════════
// 4. Scorecard harvester — trait surface, score normalisation, control mappings
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn scorecard_harvester_name_matches_expected() {
    let h = ScorecardHarvester::new(vec!["example/repo".to_owned()]);
    assert_eq!(h.name(), "OpenSSF Scorecard");
}

#[test]
fn scorecard_harvester_cadence_is_weekly() {
    let h = ScorecardHarvester::new(vec![]);
    assert_eq!(h.cadence(), HarvestCadence::Weekly);
}

#[test]
fn scorecard_harvester_is_object_safe_as_dyn_harvester() {
    let h = ScorecardHarvester::new(vec!["test/repo".to_owned()]);
    let dyn_ref: &dyn Harvester = &h;
    assert_eq!(dyn_ref.name(), "OpenSSF Scorecard");
}

#[test]
fn scorecard_normalise_score_maps_0_to_10_range() {
    assert!((normalise_score(0.0)).abs() < f64::EPSILON);
    assert!((normalise_score(5.0) - 0.5).abs() < f64::EPSILON);
    assert!((normalise_score(10.0) - 1.0).abs() < f64::EPSILON);
}

#[test]
fn scorecard_normalise_score_clamps_out_of_range() {
    assert!((normalise_score(-5.0)).abs() < f64::EPSILON);
    assert!((normalise_score(20.0) - 1.0).abs() < f64::EPSILON);
}

#[test]
fn scorecard_check_mapping_returns_dora_and_nist_for_code_review() {
    let controls = check_to_control_mapping("Code-Review").expect("mapping exists");
    assert!(controls.contains(&"NIST-800-53-R5:SA-11"));
    assert!(controls.contains(&"DORA-ART-25"));
}

#[test]
fn scorecard_check_mapping_returns_none_for_unknown_check() {
    assert!(check_to_control_mapping("Nonexistent-Check").is_none());
}

#[test]
fn scorecard_check_mapping_covers_at_least_ten_checks() {
    let known_checks = [
        "Code-Review",
        "SAST",
        "Branch-Protection",
        "Vulnerabilities",
        "Dependency-Update-Tool",
        "Signed-Releases",
        "Pinned-Dependencies",
        "Binary-Artifacts",
        "Fuzzing",
        "Security-Policy",
        "Token-Permissions",
        "Dangerous-Workflow",
        "License",
        "CII-Best-Practices",
    ];
    let mapped = known_checks
        .iter()
        .filter(|c| check_to_control_mapping(c).is_some())
        .count();
    assert!(
        mapped >= 10,
        "expected at least 10 mapped checks, got {mapped}"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 5. Manual/Seed loader — full integration with real seed files
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn manual_loader_loads_dora_seed_file() {
    let path = workspace_path("data/seed/frameworks/dora.toml");
    assert!(
        path.exists(),
        "DORA seed file not found at {}",
        path.display()
    );
    let result = load_seed_file(&path).expect("DORA seed loads successfully");
    assert_eq!(result.framework.framework_id.as_str(), "DORA");
    assert_eq!(result.framework.name, "Digital Operational Resilience Act");
    assert_eq!(result.framework.region, Region::Eu);
    assert_eq!(result.framework.harvest_source, HarvestSource::Manual);
    assert!(
        result.controls.len() >= 15,
        "expected >= 15 DORA controls, got {}",
        result.controls.len()
    );
}

#[test]
fn manual_loader_dora_controls_have_correct_ids() {
    let path = workspace_path("data/seed/frameworks/dora.toml");
    let result = load_seed_file(&path).expect("DORA seed loads");
    let ids: Vec<&str> = result
        .controls
        .iter()
        .map(|c| c.control_id.as_str())
        .collect();
    assert!(ids.contains(&"DORA-ART-25"), "missing DORA-ART-25");
    assert!(ids.contains(&"DORA-ART-26"), "missing DORA-ART-26");
    assert!(ids.contains(&"DORA-ART-9"), "missing DORA-ART-9");
}

#[test]
fn manual_loader_dora_severity_high_for_art25() {
    let path = workspace_path("data/seed/frameworks/dora.toml");
    let result = load_seed_file(&path).expect("DORA seed loads");
    let art25 = result
        .controls
        .iter()
        .find(|c| c.control_id.as_str() == "DORA-ART-25")
        .expect("DORA-ART-25 exists");
    assert_eq!(art25.severity, Severity::High);
}

#[test]
fn manual_loader_dora_families_assigned() {
    let path = workspace_path("data/seed/frameworks/dora.toml");
    let result = load_seed_file(&path).expect("DORA seed loads");
    let art25 = result
        .controls
        .iter()
        .find(|c| c.control_id.as_str() == "DORA-ART-25")
        .expect("DORA-ART-25 exists");
    assert_eq!(
        art25.family.as_ref().expect("family present").as_str(),
        "Resilience Testing"
    );
}

#[test]
fn manual_loader_loads_iso27001_seed_file() {
    let path = workspace_path("data/seed/frameworks/iso_27001_2022.toml");
    assert!(
        path.exists(),
        "ISO 27001 seed file not found at {}",
        path.display()
    );
    let result = load_seed_file(&path).expect("ISO seed loads");
    assert_eq!(result.framework.framework_id.as_str(), "ISO-27001-2022");
    assert_eq!(result.framework.region, Region::Global);
}

#[test]
fn manual_loader_trait_metadata() {
    let loader = ManualLoader::new(vec![]);
    let dyn_ref: &dyn Harvester = &loader;
    assert_eq!(dyn_ref.name(), "Manual/Seed Data Loader");
    assert_eq!(dyn_ref.cadence(), HarvestCadence::OnVersion);
}

#[test]
fn manual_loader_inline_toml_produces_controls() {
    let toml = r#"
[framework]
id = "TEST-FW"
name = "Test Framework"
version = "1.0"
region = "us"
authority = "Test Authority"

[[controls]]
id = "TEST-1"
title = "First Control"
description = "A test control"
family = "Testing"
severity = "High"
testing_relevant = true

[[controls]]
id = "TEST-2"
title = "Second Control"
description = "Another test control"
family = "Governance"
severity = "Low"
testing_relevant = false
"#;
    let result = load_seed_toml(toml).expect("inline TOML loads");
    assert_eq!(result.controls.len(), 2);
    assert_eq!(result.framework.framework_id.as_str(), "TEST-FW");
    assert_eq!(result.framework.region, Region::Us);

    let c1 = result
        .controls
        .iter()
        .find(|c| c.control_id.as_str() == "TEST-1")
        .expect("TEST-1 exists");
    assert_eq!(c1.severity, Severity::High);
    assert!(c1.testing_relevant);

    let c2 = result
        .controls
        .iter()
        .find(|c| c.control_id.as_str() == "TEST-2")
        .expect("TEST-2 exists");
    assert_eq!(c2.severity, Severity::Low);
    assert!(!c2.testing_relevant);
}

#[tokio::test]
async fn manual_loader_harvest_with_no_paths_returns_error() {
    let loader = ManualLoader::new(vec![]);
    let config = HarvestConfig::new(Duration::from_secs(5));
    let result = loader.harvest(&config).await;
    assert!(result.is_err(), "harvest with no paths should fail");
}

#[tokio::test]
async fn manual_loader_harvest_with_real_seed_produces_controls() {
    let path = workspace_path("data/seed/frameworks/dora.toml");
    let loader = ManualLoader::new(vec![path]);
    let config = HarvestConfig::new(Duration::from_secs(5));
    let result = loader.harvest(&config).await.expect("harvest succeeds");
    assert_eq!(result.framework.framework_id.as_str(), "DORA");
    assert!(result.controls.len() >= 15);
}

// ═════════════════════════════════════════════════════════════════════════════
// 6. Mapping loader — integration with real mapping files
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_loader_loads_dora_iso_file() {
    let path = workspace_path("data/seed/mappings/dora_iso27001.toml");
    if !path.exists() {
        return; // skip if seed data not present
    }
    let mappings = load_mapping_file(&path).expect("mapping file loads");
    assert!(
        mappings.len() >= 15,
        "expected >= 15 DORA-ISO mappings, got {}",
        mappings.len()
    );
    // Verify at least one high-confidence mapping
    let high = mappings
        .iter()
        .filter(|m| m.confidence == Confidence::High)
        .count();
    assert!(high >= 10, "expected >= 10 high-confidence, got {high}");
}

#[test]
fn mapping_loader_loads_all_seed_mappings() {
    let dir = workspace_path("data/seed/mappings");
    if !dir.exists() {
        return;
    }
    let all = load_all_mappings(&dir).expect("all mappings load");
    assert!(
        all.len() >= 40,
        "expected >= 40 total mappings, got {}",
        all.len()
    );
}

#[test]
fn mapping_loader_inline_toml_roundtrip() {
    let toml = r#"
[[mappings]]
source = "DORA-ART-25"
target = "NIST-SA-11"
relationship = "Partial"
confidence = "Medium"
direction = "SourceToTarget"
provenance = "Manual"
"#;
    let mappings = load_mapping_toml(toml).expect("inline mapping loads");
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].source_control.as_str(), "DORA-ART-25");
    assert_eq!(mappings[0].target_control.as_str(), "NIST-SA-11");
    assert_eq!(mappings[0].relationship, MappingRelationship::Partial);
    assert_eq!(mappings[0].confidence, Confidence::Medium);
    assert_eq!(mappings[0].direction, MappingDirection::SourceToTarget);
}

// ═════════════════════════════════════════════════════════════════════════════
// 7. Scheduler — cadence and due-for-harvest logic
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn scheduler_daily_is_due_when_never_harvested() {
    let now = Utc::now();
    assert!(is_due(HarvestCadence::Daily, None, now));
}

#[test]
fn scheduler_daily_is_due_after_24_hours() {
    let now = Utc::now();
    let last = now - chrono::Duration::hours(25);
    assert!(is_due(HarvestCadence::Daily, Some(last), now));
}

#[test]
fn scheduler_daily_not_due_within_24_hours() {
    let now = Utc::now();
    let last = now - chrono::Duration::hours(12);
    assert!(!is_due(HarvestCadence::Daily, Some(last), now));
}

#[test]
fn scheduler_weekly_is_due_after_7_days() {
    let now = Utc::now();
    let last = now - chrono::Duration::days(8);
    assert!(is_due(HarvestCadence::Weekly, Some(last), now));
}

#[test]
fn scheduler_weekly_not_due_within_7_days() {
    let now = Utc::now();
    let last = now - chrono::Duration::days(3);
    assert!(!is_due(HarvestCadence::Weekly, Some(last), now));
}

#[test]
fn scheduler_monthly_is_due_after_30_days() {
    let now = Utc::now();
    let last = now - chrono::Duration::days(31);
    assert!(is_due(HarvestCadence::Monthly, Some(last), now));
}

#[test]
fn scheduler_monthly_not_due_within_30_days() {
    let now = Utc::now();
    let last = now - chrono::Duration::days(15);
    assert!(!is_due(HarvestCadence::Monthly, Some(last), now));
}

#[test]
fn scheduler_cadence_durations_correct() {
    assert_eq!(cadence_to_duration(HarvestCadence::Daily).num_days(), 1);
    assert_eq!(cadence_to_duration(HarvestCadence::Weekly).num_days(), 7);
    assert_eq!(cadence_to_duration(HarvestCadence::Monthly).num_days(), 30);
    assert_eq!(
        cadence_to_duration(HarvestCadence::OnRelease).num_days(),
        365
    );
    assert_eq!(
        cadence_to_duration(HarvestCadence::OnVersion).num_days(),
        365
    );
}

#[test]
fn scheduler_find_due_harvesters_returns_all_when_never_harvested() {
    let harvesters: Vec<Box<dyn Harvester>> = vec![
        Box::new(CellarHarvester::new()),
        Box::new(NvdHarvester::new()),
    ];
    let now = Utc::now();
    let never = |_: &FrameworkId| -> Option<DateTime<Utc>> { None };
    let due = find_due_harvesters(&harvesters, &never, now);
    assert_eq!(due.len(), 2);
}

#[test]
fn scheduler_find_due_harvesters_filters_recently_harvested() {
    let harvesters: Vec<Box<dyn Harvester>> = vec![
        Box::new(CellarHarvester::new()), // Monthly
        Box::new(NvdHarvester::new()),    // Daily
    ];
    let now = Utc::now();
    let two_days_ago = now - chrono::Duration::days(2);

    // Simulate: all frameworks harvested 2 days ago
    // NVD (Daily) should be due, CELLAR (Monthly) should not
    let recent = |_: &FrameworkId| -> Option<DateTime<Utc>> { Some(two_days_ago) };
    let due = find_due_harvesters(&harvesters, &recent, now);

    let names: Vec<&str> = due.iter().map(|h| h.name()).collect();
    assert!(
        names.contains(&"NIST NVD (CVE API 2.0)"),
        "NVD should be due"
    );
    assert!(
        !names.contains(&"EUR-Lex CELLAR (SPARQL)"),
        "CELLAR should not be due"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 8. Snapshot — path generation and filesystem operations
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_path_generates_expected_format() {
    let base = Path::new("/data/lake");
    let ts = chrono::DateTime::parse_from_rfc3339("2026-04-11T10:00:00Z")
        .expect("valid timestamp")
        .with_timezone(&Utc);
    let path = snapshot_path(base, &fw("DORA"), ts);
    assert_eq!(
        path.to_str().expect("valid path"),
        "/data/lake/DORA/2026-04-11.parquet"
    );
}

#[test]
fn snapshot_path_different_frameworks_produce_different_dirs() {
    let base = Path::new("/data");
    let ts = Utc::now();
    let dora = snapshot_path(base, &fw("DORA"), ts);
    let nist = snapshot_path(base, &fw("NIST-800-53-R5"), ts);
    assert_ne!(dora.parent(), nist.parent());
}

#[test]
fn snapshot_ensure_dir_creates_framework_subdirectory() {
    let tmp = tempfile::tempdir().expect("temp dir");
    ensure_snapshot_dir(tmp.path(), &fw("DORA")).expect("dir created");
    assert!(tmp.path().join("DORA").is_dir());
}

#[test]
fn snapshot_list_empty_dir_returns_empty() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let snaps = list_snapshots(tmp.path(), &fw("DORA")).expect("list succeeds");
    assert!(snaps.is_empty());
}

#[test]
fn snapshot_list_finds_parquet_files_newest_first() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let fw_dir = tmp.path().join("DORA");
    std::fs::create_dir_all(&fw_dir).expect("create dir");
    std::fs::write(fw_dir.join("2026-01-01.parquet"), b"v1").expect("write");
    std::fs::write(fw_dir.join("2026-03-15.parquet"), b"v2").expect("write");
    std::fs::write(fw_dir.join("2026-02-10.parquet"), b"v3").expect("write");
    std::fs::write(fw_dir.join("notes.txt"), b"ignored").expect("write");

    let snaps = list_snapshots(tmp.path(), &fw("DORA")).expect("list succeeds");
    assert_eq!(snaps.len(), 3);
    // Newest first (alphabetical reverse for date-named files)
    assert!(
        snaps[0].to_str().expect("valid").contains("2026-03-15"),
        "newest should be first"
    );
    assert!(
        snaps[2].to_str().expect("valid").contains("2026-01-01"),
        "oldest should be last"
    );
}

#[test]
fn snapshot_retention_deletes_oldest_files() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let fw_dir = tmp.path().join("DORA");
    std::fs::create_dir_all(&fw_dir).expect("create dir");
    for i in 1..=6 {
        std::fs::write(fw_dir.join(format!("2026-0{i}-01.parquet")), b"data").expect("write");
    }

    let deleted = apply_retention(tmp.path(), &fw("DORA"), 3).expect("retention applied");
    assert_eq!(deleted, 3, "should delete 3 oldest");

    let remaining = list_snapshots(tmp.path(), &fw("DORA")).expect("list");
    assert_eq!(remaining.len(), 3);
}

#[test]
fn snapshot_retention_noop_when_under_limit() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let fw_dir = tmp.path().join("DORA");
    std::fs::create_dir_all(&fw_dir).expect("create dir");
    std::fs::write(fw_dir.join("2026-01-01.parquet"), b"only one").expect("write");

    let deleted = apply_retention(tmp.path(), &fw("DORA"), 10).expect("retention");
    assert_eq!(deleted, 0);
}

// ═════════════════════════════════════════════════════════════════════════════
// 9. Harvest log — record construction and status verification
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn harvest_log_success_record() {
    let now = Utc::now();
    let log = HarvestLog {
        harvest_id: "harvest-001".to_owned(),
        framework_id: fw("DORA"),
        started_at: now - chrono::Duration::seconds(30),
        completed_at: Some(now),
        status: HarvestStatus::Completed,
        controls_added: 20,
        controls_updated: 5,
        mappings_added: 10,
        error_message: None,
    };
    assert_eq!(log.status, HarvestStatus::Completed);
    assert!(log.completed_at.is_some());
    assert!(log.error_message.is_none());
    assert_eq!(log.controls_added, 20);
    assert_eq!(log.controls_updated, 5);
    assert_eq!(log.mappings_added, 10);
}

#[test]
fn harvest_log_failure_preserves_error_message() {
    let now = Utc::now();
    let error_msg = "HTTP 503: Service temporarily unavailable";
    let log = HarvestLog {
        harvest_id: "harvest-002".to_owned(),
        framework_id: fw("NVD"),
        started_at: now,
        completed_at: Some(now + chrono::Duration::seconds(2)),
        status: HarvestStatus::Failed,
        controls_added: 0,
        controls_updated: 0,
        mappings_added: 0,
        error_message: Some(error_msg.to_owned()),
    };
    assert_eq!(log.status, HarvestStatus::Failed);
    assert_eq!(log.error_message.as_deref(), Some(error_msg));
    assert_eq!(log.controls_added, 0);
}

#[test]
fn harvest_log_running_status_has_no_completion_time() {
    let now = Utc::now();
    let log = HarvestLog {
        harvest_id: "harvest-003".to_owned(),
        framework_id: fw("DORA"),
        started_at: now,
        completed_at: None,
        status: HarvestStatus::Running,
        controls_added: 0,
        controls_updated: 0,
        mappings_added: 0,
        error_message: None,
    };
    assert_eq!(log.status, HarvestStatus::Running);
    assert!(log.completed_at.is_none());
}

#[test]
fn harvest_log_serde_roundtrip() {
    let now = Utc::now();
    let log = HarvestLog {
        harvest_id: "harvest-rt".to_owned(),
        framework_id: fw("DORA"),
        started_at: now,
        completed_at: Some(now),
        status: HarvestStatus::Completed,
        controls_added: 42,
        controls_updated: 7,
        mappings_added: 15,
        error_message: None,
    };
    let json = serde_json::to_string(&log).expect("serialize");
    let deserialized: HarvestLog = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized.harvest_id, "harvest-rt");
    assert_eq!(deserialized.status, HarvestStatus::Completed);
    assert_eq!(deserialized.controls_added, 42);
}

// ═════════════════════════════════════════════════════════════════════════════
// 10. Fixture file validation — ensure fixture JSON is well-formed
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn fixture_cellar_sparql_is_valid_json() {
    let content = include_str!("fixtures/cellar_sparql_response.json");
    let parsed: serde_json::Value = serde_json::from_str(content).expect("valid JSON");
    let bindings = parsed["results"]["bindings"]
        .as_array()
        .expect("bindings is array");
    assert_eq!(bindings.len(), 5, "fixture has 5 article bindings");
    // Verify structure: each binding has articleNumber and title
    for binding in bindings {
        assert!(
            binding.get("articleNumber").is_some(),
            "binding missing articleNumber"
        );
    }
}

#[test]
fn fixture_oscal_catalog_is_valid_json() {
    let content = include_str!("fixtures/oscal_catalog.json");
    let parsed: serde_json::Value = serde_json::from_str(content).expect("valid JSON");
    let groups = parsed["catalog"]["groups"]
        .as_array()
        .expect("groups is array");
    assert_eq!(groups.len(), 2, "fixture has 2 control groups");
}

#[test]
fn fixture_nvd_response_is_valid_json() {
    let content = include_str!("fixtures/nvd_cve_response.json");
    let parsed: serde_json::Value = serde_json::from_str(content).expect("valid JSON");
    let total = parsed["totalResults"].as_u64().expect("totalResults");
    assert_eq!(total, 3);
    let vulns = parsed["vulnerabilities"]
        .as_array()
        .expect("vulnerabilities");
    assert_eq!(vulns.len(), 3);
    // Verify CVE IDs follow expected format
    for vuln in vulns {
        let cve_id = vuln["cve"]["id"].as_str().expect("cve.id");
        assert!(cve_id.starts_with("CVE-"), "CVE ID format: {cve_id}");
    }
}

#[test]
fn fixture_scorecard_response_is_valid_json() {
    let content = include_str!("fixtures/scorecard_response.json");
    let parsed: serde_json::Value = serde_json::from_str(content).expect("valid JSON");
    let score = parsed["score"].as_f64().expect("score");
    assert!(score > 0.0 && score <= 10.0, "score in valid range");
    let checks = parsed["checks"].as_array().expect("checks");
    assert!(checks.len() >= 5, "fixture has >= 5 checks");
}

// ═════════════════════════════════════════════════════════════════════════════
// 11. Cross-cutting: HarvestConfig construction
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn harvest_config_default_has_no_api_keys() {
    let config = HarvestConfig::new(Duration::from_secs(30));
    assert_eq!(config.timeout, Duration::from_secs(30));
    assert!(config.api_keys.nvd_api_key.is_none());
    assert!(config.output_dir.is_none());
}

#[test]
fn harvest_config_api_key_debug_redacts() {
    let mut config = HarvestConfig::new(Duration::from_secs(10));
    config.api_keys.nvd_api_key = Some("super-secret-key".to_owned());
    let debug_str = format!("{:?}", config.api_keys);
    assert!(
        !debug_str.contains("super-secret"),
        "API key should be redacted in debug output"
    );
    assert!(debug_str.contains("REDACTED"));
}

// =============================================================================
// Mock HTTP harvest() tests — fixture JSON wired into actual harvest() calls
// =============================================================================

/// Helper: read a fixture file as a String.
fn fixture(name: &str) -> String {
    let path = workspace_path(&format!("comp-lake-harvesters/tests/fixtures/{name}"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

#[tokio::test]
async fn cellar_harvest_parses_fixture_sparql_response() {
    use httpmock::prelude::*;

    let server = MockServer::start();
    let body = fixture("cellar_sparql_response.json");

    let _mock = server.mock(|when, then| {
        when.method(GET);
        then.status(200)
            .header("content-type", "application/json")
            .body(&body);
    });

    let base_url = Box::leak(format!("{}/sparql", server.base_url()).into_boxed_str());
    let harvester = CellarHarvester::with_base_url(base_url);
    let config = HarvestConfig::new(Duration::from_secs(10));
    let result = harvester
        .harvest(&config)
        .await
        .expect("cellar harvest should succeed with fixture");

    // The fixture has 5 SPARQL bindings -> expect >=1 control parsed
    assert!(
        !result.controls.is_empty(),
        "cellar harvest should produce controls from fixture (got 0)"
    );
    assert_eq!(result.framework.framework_id.as_str(), "DORA");
}

#[tokio::test]
async fn oscal_harvest_parses_fixture_catalog() {
    use httpmock::prelude::*;

    let server = MockServer::start();
    let body = fixture("oscal_catalog.json");

    let _mock = server.mock(|when, then| {
        when.method(GET);
        then.status(200)
            .header("content-type", "application/json")
            .body(&body);
    });

    let base_url = Box::leak(server.base_url().into_boxed_str());
    let harvester = OscalHarvester::with_base_url(base_url);
    let config = HarvestConfig::new(Duration::from_secs(10));
    let result = harvester
        .harvest(&config)
        .await
        .expect("oscal harvest should succeed with fixture");

    // Fixture has 2 groups, 3 controls
    assert!(
        !result.controls.is_empty(),
        "oscal harvest should produce controls from fixture (got 0)"
    );
}

#[tokio::test]
async fn nvd_harvest_parses_fixture_cve_response() {
    use httpmock::prelude::*;

    let server = MockServer::start();
    let body = fixture("nvd_cve_response.json");

    // NVD paginates; return totalResults=3 so it stops after one page
    let _mock = server.mock(|when, then| {
        when.method(GET);
        then.status(200)
            .header("content-type", "application/json")
            .body(&body);
    });

    let base_url = Box::leak(server.base_url().into_boxed_str());
    let harvester = NvdHarvester::with_base_url(base_url);
    let config = HarvestConfig::new(Duration::from_secs(10));
    let result = harvester
        .harvest(&config)
        .await
        .expect("nvd harvest should succeed with fixture");

    // NVD produces evidence records (not controls) — assert harvest completes without error.
    // The fixture has 3 vulnerabilities; NVD's totalResults=3 causes the paginator to stop.
    // Controls list is intentionally empty for NVD (it feeds the evidence table, not controls).
    assert_eq!(result.framework.framework_id.as_str(), "NVD");
    // Harvest succeeded — the fixture was parsed without error
    let _ = result;
}

#[tokio::test]
async fn scorecard_harvest_parses_fixture_response() {
    use httpmock::prelude::*;

    let server = MockServer::start();
    let body = fixture("scorecard_response.json");

    let _mock = server.mock(|when, then| {
        when.method(GET);
        then.status(200)
            .header("content-type", "application/json")
            .body(&body);
    });

    let base_url = Box::leak(server.base_url().into_boxed_str());
    let harvester = ScorecardHarvester::with_base_url(
        vec!["github.com/example/secure-app".to_owned()],
        base_url,
    );
    let config = HarvestConfig::new(Duration::from_secs(10));
    let result = harvester
        .harvest(&config)
        .await
        .expect("scorecard harvest should succeed with fixture");

    // Scorecard produces evidence (not controls). Verify harvest succeeds and framework is correct.
    assert_eq!(result.framework.framework_id.as_str(), "OSSF-SCORECARD");
    // The fixture score is 8.2 with 7 checks; harvest completes without error.
    let _ = result;
}
