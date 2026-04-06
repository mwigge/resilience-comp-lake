/// Shared test fixtures for use across crates.
///
/// Import via `comp_lake_core::test_fixtures` in dev-dependencies.
use chrono::{DateTime, Utc};

use crate::models::control::{Control, ControlFamily, ControlId, Severity};
use crate::models::evidence::{
    Evidence, EvidenceId, EvidenceResult, EvidenceType, SourceSystem,
};
use crate::models::framework::{Framework, FrameworkId, HarvestSource, Region};
use crate::models::freshness::compute_expires_at;
use crate::models::org::EntityId;

/// Create a test DORA framework.
///
/// # Panics
///
/// Panics if hardcoded IDs are invalid (should never happen).
#[must_use]
pub fn test_framework() -> Framework {
    Framework::builder(FrameworkId::new("DORA").expect("valid"), "DORA")
        .version("2022/2554")
        .region(Region::Eu)
        .authority("EU/EP")
        .harvest_source(HarvestSource::Manual)
        .build()
}

/// Create a test control with the given ID and testing-relevance.
///
/// # Panics
///
/// Panics if the provided ID is invalid.
#[must_use]
pub fn test_control(id: &str, testing_relevant: bool) -> Control {
    Control::builder(
        ControlId::new(id).expect("valid control ID"),
        FrameworkId::new("DORA").expect("valid framework ID"),
        format!("Control {id}"),
    )
    .severity(Severity::High)
    .family(ControlFamily::new("Testing"))
    .testing_relevant(testing_relevant)
    .build()
}

/// Create a test evidence record.
///
/// # Panics
///
/// Panics if the provided control ID is invalid.
#[must_use]
pub fn test_evidence(
    entity: &str,
    control_id: &str,
    result: EvidenceResult,
    now: DateTime<Utc>,
) -> Evidence {
    let et = EvidenceType::ChaosExperiment;
    Evidence {
        evidence_id: EvidenceId::new(),
        entity_id: EntityId::new(entity),
        control_id: ControlId::new(control_id).expect("valid control ID"),
        evidence_type: et,
        source_system: SourceSystem::new("tumult"),
        result,
        score: None,
        metadata: serde_json::Value::Null,
        observed_at: now,
        expires_at: compute_expires_at(now, &et),
    }
}
