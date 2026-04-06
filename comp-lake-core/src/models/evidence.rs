use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::control::ControlId;
use super::org::EntityId;

/// Unique identifier for an evidence record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EvidenceId(Uuid);

impl EvidenceId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    #[must_use]
    pub fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    #[must_use]
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for EvidenceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for EvidenceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Category of evidence that demonstrates compliance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceType {
    ChaosExperiment,
    GameDay,
    PenTest,
    VulnScan,
    DoraMetric,
    Scorecard,
    AuditFinding,
    UnitTest,
    IntegrationTest,
}

/// Outcome of an evidence assessment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceResult {
    Pass,
    Fail,
    Partial,
}

impl EvidenceResult {
    /// Numeric score: Pass=1.0, Fail=0.0, Partial=0.5.
    #[must_use]
    pub fn as_score(self) -> f64 {
        match self {
            Self::Pass => 1.0,
            Self::Fail => 0.0,
            Self::Partial => 0.5,
        }
    }
}

/// Source system that produced the evidence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceSystem(String);

impl SourceSystem {
    /// # Panics
    ///
    /// Panics if `name` is empty.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(!name.is_empty(), "SourceSystem must not be empty");
        Self(name)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SourceSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A piece of evidence linking an entity to a control.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub evidence_id: EvidenceId,
    pub entity_id: EntityId,
    pub control_id: ControlId,
    pub evidence_type: EvidenceType,
    pub source_system: SourceSystem,
    pub result: EvidenceResult,
    pub score: Option<f64>,
    pub metadata: serde_json::Value,
    pub observed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_result_scores() {
        assert!((EvidenceResult::Pass.as_score() - 1.0).abs() < f64::EPSILON);
        assert!((EvidenceResult::Fail.as_score() - 0.0).abs() < f64::EPSILON);
        assert!((EvidenceResult::Partial.as_score() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn evidence_id_display() {
        let id = EvidenceId::new();
        let s = id.to_string();
        assert!(!s.is_empty());
    }

    #[test]
    fn evidence_type_serde_roundtrip() {
        for et in [
            EvidenceType::ChaosExperiment,
            EvidenceType::GameDay,
            EvidenceType::PenTest,
            EvidenceType::VulnScan,
            EvidenceType::DoraMetric,
            EvidenceType::Scorecard,
            EvidenceType::AuditFinding,
            EvidenceType::UnitTest,
            EvidenceType::IntegrationTest,
        ] {
            let json = serde_json::to_string(&et).unwrap();
            let deserialized: EvidenceType = serde_json::from_str(&json).unwrap();
            assert_eq!(et, deserialized);
        }
    }

    #[test]
    fn evidence_serde_roundtrip() {
        let ev = Evidence {
            evidence_id: EvidenceId::new(),
            entity_id: EntityId::new("team-alpha"),
            control_id: ControlId::new("DORA-ART-25").unwrap(),
            evidence_type: EvidenceType::ChaosExperiment,
            source_system: SourceSystem::new("tumult"),
            result: EvidenceResult::Pass,
            score: Some(1.0),
            metadata: serde_json::json!({"experiment": "db-failover"}),
            observed_at: Utc::now(),
            expires_at: Utc::now(),
        };

        let json = serde_json::to_string(&ev).unwrap();
        let deserialized: Evidence = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, deserialized);
    }
}
