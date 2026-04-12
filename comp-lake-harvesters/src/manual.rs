use std::path::Path;

use chrono::Utc;
use serde::Deserialize;

use comp_lake_core::models::control::{Control, ControlFamily, ControlId, Severity};
use comp_lake_core::models::framework::{Framework, FrameworkId, HarvestSource, Region};

use crate::harvester::{HarvestCadence, HarvestError, HarvestResult, Harvester};

/// Seed file format: a TOML file describing a framework and its controls.
#[derive(Debug, Deserialize)]
pub struct SeedFile {
    pub framework: SeedFramework,
    #[serde(default)]
    pub controls: Vec<SeedControl>,
}

#[derive(Debug, Deserialize)]
pub struct SeedFramework {
    pub id: String,
    pub name: String,
    pub version: String,
    pub region: String,
    pub authority: String,
}

#[derive(Debug, Deserialize)]
pub struct SeedControl {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub family: String,
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default = "default_true")]
    pub testing_relevant: bool,
    #[serde(default)]
    pub article_ref: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
}

fn default_severity() -> String {
    "Moderate".to_owned()
}

fn default_true() -> bool {
    true
}

/// Loader for seed/manual framework data from TOML files.
pub struct ManualLoader {
    framework_ids: Vec<FrameworkId>,
    seed_paths: Vec<std::path::PathBuf>,
}

impl ManualLoader {
    /// # Panics
    ///
    /// Panics if seed file framework IDs are invalid (should never happen with valid TOML).
    #[must_use]
    pub fn new(seed_paths: Vec<std::path::PathBuf>) -> Self {
        Self {
            framework_ids: Vec::new(), // populated on first harvest
            seed_paths,
        }
    }
}

impl Harvester for ManualLoader {
    fn name(&self) -> &'static str {
        "Manual/Seed Data Loader"
    }

    fn frameworks(&self) -> &[FrameworkId] {
        &self.framework_ids
    }

    fn cadence(&self) -> HarvestCadence {
        HarvestCadence::OnVersion
    }

    fn harvest<'a>(
        &'a self,
        _config: &'a crate::harvester::HarvestConfig,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<HarvestResult, HarvestError>> + Send + 'a>,
    > {
        Box::pin(async move {
            if self.seed_paths.is_empty() {
                return Err(HarvestError::Other("no seed paths configured".to_owned()));
            }
            load_seed_file(&self.seed_paths[0])
        })
    }
}

/// Load a single seed TOML file and produce a `HarvestResult`.
///
/// # Errors
///
/// Returns `HarvestError` if the file can't be read or parsed.
pub fn load_seed_file(path: &Path) -> Result<HarvestResult, HarvestError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| HarvestError::Other(format!("failed to read {}: {e}", path.display())))?;
    load_seed_toml(&content)
}

/// Parse a seed TOML string into a `HarvestResult`.
///
/// # Errors
///
/// Returns `HarvestError::Parse` if the TOML is invalid.
pub fn load_seed_toml(toml_str: &str) -> Result<HarvestResult, HarvestError> {
    let seed: SeedFile =
        toml::from_str(toml_str).map_err(|e| HarvestError::Parse(e.to_string()))?;

    let framework_id =
        FrameworkId::new(&seed.framework.id).map_err(|e| HarvestError::Parse(e.to_string()))?;

    let region = match seed.framework.region.to_lowercase().as_str() {
        "eu" => Region::Eu,
        "us" => Region::Us,
        _ => Region::Global,
    };

    let framework = Framework::builder(framework_id.clone(), &seed.framework.name)
        .version(&seed.framework.version)
        .region(region)
        .authority(&seed.framework.authority)
        .harvest_source(HarvestSource::Manual)
        .last_harvested(Utc::now())
        .build();

    let controls = seed
        .controls
        .iter()
        .filter_map(|sc| {
            let control_id = ControlId::new(&sc.id).ok()?;
            let severity = match sc.severity.to_lowercase().as_str() {
                "high" => Severity::High,
                "low" => Severity::Low,
                _ => Severity::Moderate,
            };

            let mut builder = Control::builder(control_id, framework_id.clone(), &sc.title)
                .severity(severity)
                .testing_relevant(sc.testing_relevant)
                .description(&sc.description);

            if !sc.family.is_empty() {
                builder = builder.family(ControlFamily::new(&sc.family));
            }
            if let Some(ref art) = sc.article_ref {
                builder = builder.article_ref(art);
            }
            if let Some(ref pid) = sc.parent_id {
                if let Ok(pid) = ControlId::new(pid) {
                    builder = builder.parent_id(pid);
                }
            }

            Some(builder.build())
        })
        .collect();

    Ok(HarvestResult {
        framework,
        controls,
        mappings: Vec::new(),
        snapshot_version: format!("seed-{}", Utc::now().format("%Y%m%d")),
        harvested_at: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PCI_SEED: &str = r#"
[framework]
id = "PCI-DSS-4"
name = "PCI DSS 4.0.1"
version = "4.0.1"
region = "global"
authority = "PCI SSC"

[[controls]]
id = "PCI-1.1.1"
title = "Network security controls are defined and understood"
description = "Processes and mechanisms for network security controls are defined and understood"
family = "Network Security"
severity = "High"
testing_relevant = true

[[controls]]
id = "PCI-6.2.4"
title = "Software engineering techniques prevent attacks"
description = "Custom software is developed following secure coding practices"
family = "Secure Development"
severity = "High"
testing_relevant = true

[[controls]]
id = "PCI-11.3.1"
title = "Internal vulnerability scans performed"
description = "Internal vulnerability scans performed at least quarterly"
family = "Vulnerability Management"
severity = "High"
testing_relevant = true

[[controls]]
id = "PCI-1.1.0"
title = "Documentation for network security"
description = "Documentation requirements for network security"
family = "Network Security"
severity = "Low"
testing_relevant = false
"#;

    const ISO_SEED: &str = r#"
[framework]
id = "ISO-27001-2022"
name = "ISO/IEC 27001:2022"
version = "2022"
region = "global"
authority = "ISO/IEC"

[[controls]]
id = "ISO-A.8.8"
title = "Management of technical vulnerabilities"
description = "Timely identification and remediation of technical vulnerabilities"
family = "Technology Controls"
severity = "High"
testing_relevant = true

[[controls]]
id = "ISO-A.5.35"
title = "Independent review of information security"
description = "Independent review of approach to managing information security"
family = "Organisational Controls"
severity = "Moderate"
testing_relevant = true
"#;

    #[test]
    fn parse_pci_seed() {
        let result = load_seed_toml(PCI_SEED).unwrap();
        assert_eq!(result.framework.framework_id.as_str(), "PCI-DSS-4");
        assert_eq!(result.controls.len(), 4);
    }

    #[test]
    fn pci_testing_relevant_count() {
        let result = load_seed_toml(PCI_SEED).unwrap();
        let relevant: Vec<_> = result
            .controls
            .iter()
            .filter(|c| c.testing_relevant)
            .collect();
        assert_eq!(relevant.len(), 3);
    }

    #[test]
    fn parse_iso_seed() {
        let result = load_seed_toml(ISO_SEED).unwrap();
        assert_eq!(result.framework.framework_id.as_str(), "ISO-27001-2022");
        assert_eq!(result.controls.len(), 2);
    }

    #[test]
    fn severity_mapping() {
        let result = load_seed_toml(PCI_SEED).unwrap();
        let high = result
            .controls
            .iter()
            .find(|c| c.control_id.as_str() == "PCI-1.1.1")
            .unwrap();
        assert_eq!(high.severity, Severity::High);

        let low = result
            .controls
            .iter()
            .find(|c| c.control_id.as_str() == "PCI-1.1.0")
            .unwrap();
        assert_eq!(low.severity, Severity::Low);
    }

    #[test]
    fn family_assignment() {
        let result = load_seed_toml(PCI_SEED).unwrap();
        let ctrl = result
            .controls
            .iter()
            .find(|c| c.control_id.as_str() == "PCI-6.2.4")
            .unwrap();
        assert_eq!(ctrl.family.as_ref().unwrap().as_str(), "Secure Development");
    }

    #[test]
    fn region_mapping() {
        let result = load_seed_toml(PCI_SEED).unwrap();
        assert_eq!(result.framework.region, Region::Global);

        let eu_seed = r#"
[framework]
id = "TEST"
name = "Test"
version = "1"
region = "eu"
authority = "Test"
"#;
        let result = load_seed_toml(eu_seed).unwrap();
        assert_eq!(result.framework.region, Region::Eu);
    }

    #[test]
    fn idempotent_loading() {
        let r1 = load_seed_toml(PCI_SEED).unwrap();
        let r2 = load_seed_toml(PCI_SEED).unwrap();
        assert_eq!(r1.controls.len(), r2.controls.len());
        for (c1, c2) in r1.controls.iter().zip(r2.controls.iter()) {
            assert_eq!(c1.control_id, c2.control_id);
        }
    }

    #[test]
    fn invalid_toml_returns_error() {
        let result = load_seed_toml("this is not valid toml {{{");
        assert!(result.is_err());
    }

    #[test]
    fn manual_loader_object_safe() {
        let loader = ManualLoader::new(vec![]);
        let dyn_ref: &dyn Harvester = &loader;
        assert_eq!(dyn_ref.name(), "Manual/Seed Data Loader");
        assert_eq!(dyn_ref.cadence(), HarvestCadence::OnVersion);
    }
}
