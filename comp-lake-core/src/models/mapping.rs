use serde::{Deserialize, Serialize};

use super::control::ControlId;

/// Relationship between two controls across frameworks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MappingRelationship {
    Equivalent,
    Partial,
    Supplements,
    DerivedFrom,
}

/// Confidence level of a cross-framework control mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    /// Numeric weight for scoring: High=1.0, Medium=0.7, Low=0.4.
    #[must_use]
    pub fn as_weight(self) -> f64 {
        match self {
            Self::High => 1.0,
            Self::Medium => 0.7,
            Self::Low => 0.4,
        }
    }
}

/// Direction of a control mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MappingDirection {
    Bidirectional,
    SourceToTarget,
}

/// Where a mapping was sourced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MappingProvenance {
    EbaMapping,
    NistOlir,
    OscalProfile,
    Manual,
}

/// A mapping between two controls in different frameworks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlMapping {
    pub source_control: ControlId,
    pub target_control: ControlId,
    pub relationship: MappingRelationship,
    pub confidence: Confidence,
    pub direction: MappingDirection,
    pub provenance: MappingProvenance,
}

impl ControlMapping {
    #[must_use]
    pub fn new(
        source_control: ControlId,
        target_control: ControlId,
        relationship: MappingRelationship,
        confidence: Confidence,
        direction: MappingDirection,
        provenance: MappingProvenance,
    ) -> Self {
        Self {
            source_control,
            target_control,
            relationship,
            confidence,
            direction,
            provenance,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_weights() {
        assert!((Confidence::High.as_weight() - 1.0).abs() < f64::EPSILON);
        assert!((Confidence::Medium.as_weight() - 0.7).abs() < f64::EPSILON);
        assert!((Confidence::Low.as_weight() - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn mapping_serde_roundtrip() {
        let mapping = ControlMapping::new(
            ControlId::new("DORA-ART-25").unwrap(),
            ControlId::new("NIST-IR-4").unwrap(),
            MappingRelationship::Equivalent,
            Confidence::High,
            MappingDirection::Bidirectional,
            MappingProvenance::NistOlir,
        );

        let json = serde_json::to_string(&mapping).unwrap();
        let deserialized: ControlMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(mapping, deserialized);
    }

    #[test]
    fn relationship_serde_roundtrip() {
        for rel in [
            MappingRelationship::Equivalent,
            MappingRelationship::Partial,
            MappingRelationship::Supplements,
            MappingRelationship::DerivedFrom,
        ] {
            let json = serde_json::to_string(&rel).unwrap();
            let deserialized: MappingRelationship = serde_json::from_str(&json).unwrap();
            assert_eq!(rel, deserialized);
        }
    }
}
