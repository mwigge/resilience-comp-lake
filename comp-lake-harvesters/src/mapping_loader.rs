use std::path::Path;

use serde::Deserialize;

use comp_lake_core::models::control::ControlId;
use comp_lake_core::models::mapping::{
    Confidence, ControlMapping, MappingDirection, MappingProvenance, MappingRelationship,
};

use crate::harvester::HarvestError;

/// TOML format for a mapping seed file.
#[derive(Debug, Deserialize)]
struct MappingFile {
    mappings: Vec<MappingSeed>,
}

#[derive(Debug, Deserialize)]
struct MappingSeed {
    source: String,
    target: String,
    relationship: String,
    confidence: String,
    direction: String,
    provenance: String,
}

/// Load control mappings from a TOML seed file.
///
/// # Errors
///
/// Returns `HarvestError` if the file can't be read or parsed.
pub fn load_mapping_file(path: &Path) -> Result<Vec<ControlMapping>, HarvestError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| HarvestError::Other(format!("failed to read {}: {e}", path.display())))?;
    load_mapping_toml(&content)
}

/// Parse mapping TOML string into `ControlMapping` records.
///
/// # Errors
///
/// Returns `HarvestError::Parse` if the TOML is invalid or contains unknown enum values.
pub fn load_mapping_toml(toml_str: &str) -> Result<Vec<ControlMapping>, HarvestError> {
    let file: MappingFile =
        toml::from_str(toml_str).map_err(|e| HarvestError::Parse(e.to_string()))?;

    file.mappings
        .into_iter()
        .map(|m| {
            let source =
                ControlId::new(&m.source).map_err(|e| HarvestError::Parse(e.to_string()))?;
            let target =
                ControlId::new(&m.target).map_err(|e| HarvestError::Parse(e.to_string()))?;

            let relationship = match m.relationship.as_str() {
                "Equivalent" => MappingRelationship::Equivalent,
                "Partial" => MappingRelationship::Partial,
                "Supplements" => MappingRelationship::Supplements,
                "DerivedFrom" => MappingRelationship::DerivedFrom,
                other => {
                    return Err(HarvestError::Parse(format!(
                        "unknown relationship: {other}"
                    )))
                }
            };

            let confidence = match m.confidence.as_str() {
                "High" => Confidence::High,
                "Medium" => Confidence::Medium,
                "Low" => Confidence::Low,
                other => return Err(HarvestError::Parse(format!("unknown confidence: {other}"))),
            };

            let direction = match m.direction.as_str() {
                "Bidirectional" => MappingDirection::Bidirectional,
                "SourceToTarget" => MappingDirection::SourceToTarget,
                other => return Err(HarvestError::Parse(format!("unknown direction: {other}"))),
            };

            let provenance = match m.provenance.as_str() {
                "EbaMapping" => MappingProvenance::EbaMapping,
                "NistOlir" => MappingProvenance::NistOlir,
                "OscalProfile" => MappingProvenance::OscalProfile,
                "Manual" => MappingProvenance::Manual,
                other => return Err(HarvestError::Parse(format!("unknown provenance: {other}"))),
            };

            Ok(ControlMapping::new(
                source,
                target,
                relationship,
                confidence,
                direction,
                provenance,
            ))
        })
        .collect()
}

/// Load all mapping files from a directory.
///
/// # Errors
///
/// Returns an error if any file can't be read or parsed.
pub fn load_all_mappings(dir: &Path) -> Result<Vec<ControlMapping>, HarvestError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut all = Vec::new();
    let entries = std::fs::read_dir(dir)
        .map_err(|e| HarvestError::Other(format!("failed to read dir {}: {e}", dir.display())))?;

    for entry in entries {
        let entry =
            entry.map_err(|e| HarvestError::Other(format!("directory entry error: {e}")))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            let mappings = load_mapping_file(&path)?;
            all.extend(mappings);
        }
    }

    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DORA_ISO_SAMPLE: &str = r#"
[[mappings]]
source = "DORA-ART-25"
target = "ISO-A.8.8"
relationship = "Equivalent"
confidence = "High"
direction = "Bidirectional"
provenance = "EbaMapping"

[[mappings]]
source = "DORA-ART-26"
target = "ISO-A.5.35"
relationship = "Partial"
confidence = "Medium"
direction = "Bidirectional"
provenance = "Manual"
"#;

    #[test]
    fn parse_mapping_toml() {
        let mappings = load_mapping_toml(DORA_ISO_SAMPLE).unwrap();
        assert_eq!(mappings.len(), 2);
    }

    #[test]
    fn mapping_fields_correct() {
        let mappings = load_mapping_toml(DORA_ISO_SAMPLE).unwrap();
        let first = &mappings[0];
        assert_eq!(first.source_control.as_str(), "DORA-ART-25");
        assert_eq!(first.target_control.as_str(), "ISO-A.8.8");
        assert_eq!(first.relationship, MappingRelationship::Equivalent);
        assert_eq!(first.confidence, Confidence::High);
        assert_eq!(first.direction, MappingDirection::Bidirectional);
        assert_eq!(first.provenance, MappingProvenance::EbaMapping);
    }

    #[test]
    fn all_confidence_levels_parsed() {
        for (level, expected) in [
            ("High", Confidence::High),
            ("Medium", Confidence::Medium),
            ("Low", Confidence::Low),
        ] {
            let toml = format!(
                r#"
[[mappings]]
source = "A"
target = "B"
relationship = "Equivalent"
confidence = "{level}"
direction = "Bidirectional"
provenance = "Manual"
"#
            );
            let mappings = load_mapping_toml(&toml).unwrap();
            assert_eq!(mappings[0].confidence, expected);
        }
    }

    #[test]
    fn unknown_relationship_errors() {
        let toml = r#"
[[mappings]]
source = "A"
target = "B"
relationship = "Unknown"
confidence = "High"
direction = "Bidirectional"
provenance = "Manual"
"#;
        assert!(load_mapping_toml(toml).is_err());
    }

    #[test]
    fn load_real_dora_iso_seed() {
        let path = std::path::Path::new("data/seed/mappings/dora_iso27001.toml");
        if path.exists() {
            let mappings = load_mapping_file(path).unwrap();
            assert!(
                mappings.len() >= 15,
                "expected >=15 mappings, got {}",
                mappings.len()
            );

            let high_count = mappings
                .iter()
                .filter(|m| m.confidence == Confidence::High)
                .count();
            assert!(
                high_count >= 10,
                "expected >=10 High confidence, got {high_count}"
            );
        }
    }

    #[test]
    fn load_real_dora_nis2_seed() {
        let path = std::path::Path::new("data/seed/mappings/dora_nis2.toml");
        if path.exists() {
            let mappings = load_mapping_file(path).unwrap();
            assert!(
                mappings.len() >= 10,
                "expected >=10 mappings, got {}",
                mappings.len()
            );
        }
    }

    #[test]
    fn load_real_dora_cra_seed() {
        let path = std::path::Path::new("data/seed/mappings/dora_cra.toml");
        if path.exists() {
            let mappings = load_mapping_file(path).unwrap();
            assert!(
                mappings.len() >= 8,
                "expected >=8 mappings, got {}",
                mappings.len()
            );
        }
    }

    #[test]
    fn load_real_dora_gdpr_seed() {
        let path = std::path::Path::new("data/seed/mappings/dora_gdpr.toml");
        if path.exists() {
            let mappings = load_mapping_file(path).unwrap();
            assert!(
                mappings.len() >= 3,
                "expected >=3 mappings, got {}",
                mappings.len()
            );
        }
    }

    #[test]
    fn load_all_from_directory() {
        let path = std::path::Path::new("data/seed/mappings");
        if path.exists() {
            let all = load_all_mappings(path).unwrap();
            // DORA↔ISO(20) + DORA↔NIS2(12) + DORA↔CRA(8) + DORA↔GDPR(4) = 44
            assert!(
                all.len() >= 40,
                "expected >=40 total mappings, got {}",
                all.len()
            );
        }
    }

    #[test]
    fn empty_directory_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mappings = load_all_mappings(dir.path()).unwrap();
        assert!(mappings.is_empty());
    }

    #[test]
    fn nonexistent_directory_returns_empty() {
        let mappings = load_all_mappings(Path::new("/nonexistent")).unwrap();
        assert!(mappings.is_empty());
    }
}
