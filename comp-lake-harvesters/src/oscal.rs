use chrono::Utc;
use serde::Deserialize;

use comp_lake_core::models::control::{Control, ControlFamily, ControlId, Severity};
use comp_lake_core::models::framework::{Framework, FrameworkId, HarvestSource, Region};

use crate::harvester::{
    HarvestCadence, HarvestConfig, HarvestError, HarvestResult, Harvester,
};

const OSCAL_800_53_URL: &str = "https://raw.githubusercontent.com/usnistgov/oscal-content/main/nist.gov/SP800-53/rev5/json/NIST_SP-800-53_rev5_catalog.json";

/// Harvester for NIST frameworks via OSCAL JSON catalogs on GitHub.
pub struct OscalHarvester {
    framework_ids: Vec<FrameworkId>,
}

impl OscalHarvester {
    #[must_use]
    pub fn new() -> Self {
        let framework_ids = ["NIST-800-53-R5", "NIST-CSF-2"]
            .iter()
            .filter_map(|id| FrameworkId::new(*id).ok())
            .collect();
        Self { framework_ids }
    }
}

impl Default for OscalHarvester {
    fn default() -> Self {
        Self::new()
    }
}

impl Harvester for OscalHarvester {
    fn name(&self) -> &'static str {
        "NIST OSCAL (GitHub)"
    }

    fn frameworks(&self) -> &[FrameworkId] {
        &self.framework_ids
    }

    fn cadence(&self) -> HarvestCadence {
        HarvestCadence::OnRelease
    }

    fn harvest<'a>(
        &'a self,
        config: &'a HarvestConfig,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<HarvestResult, HarvestError>> + Send + 'a>,
    > {
        Box::pin(harvest_oscal(config))
    }
}

async fn harvest_oscal(config: &HarvestConfig) -> Result<HarvestResult, HarvestError> {
    let response = config
        .http_client
        .get(OSCAL_800_53_URL)
        .timeout(config.timeout)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(HarvestError::Api {
            status: response.status().as_u16(),
            message: response.text().await.unwrap_or_default(),
        });
    }

    let body: OscalCatalog = response
        .json()
        .await
        .map_err(|e| HarvestError::Parse(e.to_string()))?;

    parse_catalog(&body)
}

/// Parse an OSCAL catalog into a `HarvestResult`.
pub(crate) fn parse_catalog(catalog: &OscalCatalog) -> Result<HarvestResult, HarvestError> {
    let framework_id =
        FrameworkId::new("NIST-800-53-R5").map_err(|e| HarvestError::Parse(e.to_string()))?;

    let framework = Framework::builder(framework_id.clone(), "NIST SP 800-53 Rev. 5")
        .version("Rev. 5")
        .region(Region::Us)
        .authority("NIST")
        .is_pivot(false)
        .harvest_source(HarvestSource::OscalGithub)
        .last_harvested(Utc::now())
        .build();

    let mut controls = Vec::new();

    for group in &catalog.catalog.groups {
        let family = &group.title;

        for ctrl in &group.controls {
            if let Some(c) = parse_control(&framework_id, family, ctrl, None) {
                controls.push(c);
            }

            // Parse enhancements (sub-controls)
            if let Some(ref enhancements) = ctrl.controls {
                for enh in enhancements {
                    if let Some(c) =
                        parse_control(&framework_id, family, enh, Some(&ctrl.id))
                    {
                        controls.push(c);
                    }
                }
            }
        }
    }

    Ok(HarvestResult {
        framework,
        controls,
        mappings: Vec::new(),
        snapshot_version: format!("oscal-{}", Utc::now().format("%Y%m%d")),
        harvested_at: Utc::now(),
    })
}

fn parse_control(
    framework_id: &FrameworkId,
    family: &str,
    ctrl: &OscalControl,
    parent_id: Option<&str>,
) -> Option<Control> {
    let control_id = ControlId::new(ctrl.id.to_uppercase().replace(' ', "-")).ok()?;
    let testing_relevant = is_testing_relevant(ctrl);
    let severity = classify_severity(ctrl);

    let mut builder = Control::builder(control_id, framework_id.clone(), &ctrl.title)
        .family(ControlFamily::new(family))
        .severity(severity)
        .testing_relevant(testing_relevant);

    if let Some(pid) = parent_id {
        if let Ok(pid) = ControlId::new(pid.to_uppercase().replace(' ', "-")) {
            builder = builder.parent_id(pid);
        }
    }

    // Extract description from prose parts
    if let Some(ref parts) = ctrl.parts {
        let desc = parts
            .iter()
            .filter(|p| p.name == "statement")
            .filter_map(|p| p.prose.as_deref())
            .collect::<Vec<_>>()
            .join(" ");
        if !desc.is_empty() {
            builder = builder.description(desc);
        }
    }

    Some(builder.build())
}

/// A control is testing-relevant if its assessment methods include "TEST".
fn is_testing_relevant(ctrl: &OscalControl) -> bool {
    if let Some(ref parts) = ctrl.parts {
        for part in parts {
            if part.name == "assessment" || part.name == "assessment-method" {
                if let Some(ref props) = part.props {
                    for prop in props {
                        if prop.name == "method" && prop.value.eq_ignore_ascii_case("TEST") {
                            return true;
                        }
                    }
                }
                // Also check prose for TEST keyword
                if let Some(ref prose) = part.prose {
                    if prose.contains("TEST") {
                        return true;
                    }
                }
            }

            // Check nested parts
            if let Some(ref sub_parts) = part.parts {
                for sub in sub_parts {
                    if sub.name == "assessment-method" || sub.name == "assessment-objective" {
                        if let Some(ref props) = sub.props {
                            for prop in props {
                                if prop.name == "method"
                                    && prop.value.eq_ignore_ascii_case("TEST")
                                {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback: certain families are inherently testing-relevant
    is_family_testing_relevant(&ctrl.id)
}

/// Known testing-relevant control families by prefix.
fn is_family_testing_relevant(control_id: &str) -> bool {
    let prefix = control_id.split('-').next().unwrap_or("");
    matches!(
        prefix.to_lowercase().as_str(),
        "ac" | "au" | "ca" | "cm" | "cp" | "ia" | "ir" | "ra" | "sa" | "sc" | "si" | "sr"
    )
}

/// Classify severity from OSCAL properties or baseline impact.
fn classify_severity(ctrl: &OscalControl) -> Severity {
    if let Some(ref props) = ctrl.props {
        for prop in props {
            if prop.name == "baseline-impact" || prop.name == "impact" {
                return match prop.value.to_uppercase().as_str() {
                    "HIGH" => Severity::High,
                    "LOW" => Severity::Low,
                    _ => Severity::Moderate,
                };
            }
        }
    }
    Severity::Moderate
}

// ── OSCAL JSON structures ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct OscalCatalog {
    pub catalog: CatalogBody,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CatalogBody {
    #[serde(default)]
    pub groups: Vec<OscalGroup>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OscalGroup {
    pub title: String,
    #[serde(default)]
    pub controls: Vec<OscalControl>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OscalControl {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub props: Option<Vec<OscalProp>>,
    #[serde(default)]
    pub parts: Option<Vec<OscalPart>>,
    #[serde(default)]
    pub controls: Option<Vec<OscalControl>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OscalProp {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OscalPart {
    pub name: String,
    #[serde(default)]
    pub prose: Option<String>,
    #[serde(default)]
    pub props: Option<Vec<OscalProp>>,
    #[serde(default)]
    pub parts: Option<Vec<OscalPart>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCK_OSCAL_CATALOG: &str = r#"{
        "catalog": {
            "uuid": "test-uuid",
            "metadata": {"title": "Test", "version": "test"},
            "groups": [
                {
                    "id": "ac",
                    "title": "Access Control",
                    "controls": [
                        {
                            "id": "ac-1",
                            "title": "Policy and Procedures",
                            "props": [
                                {"name": "label", "value": "AC-1"},
                                {"name": "baseline-impact", "value": "LOW"}
                            ],
                            "parts": [
                                {"name": "statement", "prose": "Develop access control policy."},
                                {
                                    "name": "assessment",
                                    "parts": [
                                        {
                                            "name": "assessment-method",
                                            "props": [{"name": "method", "value": "EXAMINE"}]
                                        }
                                    ]
                                }
                            ]
                        },
                        {
                            "id": "ac-2",
                            "title": "Account Management",
                            "props": [
                                {"name": "label", "value": "AC-2"},
                                {"name": "baseline-impact", "value": "HIGH"}
                            ],
                            "parts": [
                                {"name": "statement", "prose": "Manage system accounts."},
                                {
                                    "name": "assessment",
                                    "parts": [
                                        {
                                            "name": "assessment-method",
                                            "props": [{"name": "method", "value": "TEST"}]
                                        }
                                    ]
                                }
                            ],
                            "controls": [
                                {
                                    "id": "ac-2.1",
                                    "title": "Automated System Account Management",
                                    "props": [
                                        {"name": "label", "value": "AC-2(1)"},
                                        {"name": "baseline-impact", "value": "HIGH"}
                                    ],
                                    "parts": [
                                        {"name": "statement", "prose": "Automate account management."}
                                    ]
                                }
                            ]
                        }
                    ]
                },
                {
                    "id": "cp",
                    "title": "Contingency Planning",
                    "controls": [
                        {
                            "id": "cp-4",
                            "title": "Contingency Plan Testing",
                            "props": [
                                {"name": "label", "value": "CP-4"},
                                {"name": "baseline-impact", "value": "MODERATE"}
                            ],
                            "parts": [
                                {"name": "statement", "prose": "Test contingency plan."},
                                {
                                    "name": "assessment",
                                    "parts": [
                                        {
                                            "name": "assessment-method",
                                            "props": [{"name": "method", "value": "TEST"}]
                                        }
                                    ]
                                }
                            ]
                        }
                    ]
                },
                {
                    "id": "pm",
                    "title": "Program Management",
                    "controls": [
                        {
                            "id": "pm-1",
                            "title": "Information Security Program Plan",
                            "props": [
                                {"name": "label", "value": "PM-1"}
                            ],
                            "parts": [
                                {"name": "statement", "prose": "Develop security plan."},
                                {
                                    "name": "assessment",
                                    "parts": [
                                        {
                                            "name": "assessment-method",
                                            "props": [{"name": "method", "value": "EXAMINE"}]
                                        }
                                    ]
                                }
                            ]
                        }
                    ]
                }
            ]
        }
    }"#;

    fn parse_mock() -> HarvestResult {
        let catalog: OscalCatalog = serde_json::from_str(MOCK_OSCAL_CATALOG).unwrap();
        parse_catalog(&catalog).unwrap()
    }

    #[test]
    fn parses_controls_from_groups() {
        let result = parse_mock();
        // ac-1, ac-2, ac-2.1 (enhancement), cp-4, pm-1 = 5 controls
        assert_eq!(result.controls.len(), 5);
    }

    #[test]
    fn control_ids_uppercase() {
        let result = parse_mock();
        let ids: Vec<_> = result.controls.iter().map(|c| c.control_id.as_str().to_owned()).collect();
        assert!(ids.contains(&"AC-1".to_owned()));
        assert!(ids.contains(&"AC-2".to_owned()));
        assert!(ids.contains(&"AC-2.1".to_owned()));
        assert!(ids.contains(&"CP-4".to_owned()));
    }

    #[test]
    fn enhancements_have_parent_id() {
        let result = parse_mock();
        let enh = result
            .controls
            .iter()
            .find(|c| c.control_id.as_str() == "AC-2.1")
            .unwrap();
        assert_eq!(enh.parent_id.as_ref().unwrap().as_str(), "AC-2");
    }

    #[test]
    fn testing_relevant_from_assessment_method() {
        let result = parse_mock();

        // AC-2 has TEST assessment method
        let ac2 = result.controls.iter().find(|c| c.control_id.as_str() == "AC-2").unwrap();
        assert!(ac2.testing_relevant);

        // CP-4 has TEST assessment method
        let cp4 = result.controls.iter().find(|c| c.control_id.as_str() == "CP-4").unwrap();
        assert!(cp4.testing_relevant);

        // PM-1 has only EXAMINE — but pm is not in the testing-relevant family list
        let pm1 = result.controls.iter().find(|c| c.control_id.as_str() == "PM-1").unwrap();
        assert!(!pm1.testing_relevant);
    }

    #[test]
    fn severity_from_baseline_impact() {
        let result = parse_mock();

        let ac1 = result.controls.iter().find(|c| c.control_id.as_str() == "AC-1").unwrap();
        assert_eq!(ac1.severity, Severity::Low);

        let ac2 = result.controls.iter().find(|c| c.control_id.as_str() == "AC-2").unwrap();
        assert_eq!(ac2.severity, Severity::High);

        let cp4 = result.controls.iter().find(|c| c.control_id.as_str() == "CP-4").unwrap();
        assert_eq!(cp4.severity, Severity::Moderate);
    }

    #[test]
    fn family_from_group_title() {
        let result = parse_mock();

        let ac2 = result.controls.iter().find(|c| c.control_id.as_str() == "AC-2").unwrap();
        assert_eq!(ac2.family.as_ref().unwrap().as_str(), "Access Control");

        let cp4 = result.controls.iter().find(|c| c.control_id.as_str() == "CP-4").unwrap();
        assert_eq!(cp4.family.as_ref().unwrap().as_str(), "Contingency Planning");
    }

    #[test]
    fn description_from_statement_prose() {
        let result = parse_mock();
        let ac2 = result.controls.iter().find(|c| c.control_id.as_str() == "AC-2").unwrap();
        assert!(ac2.description.contains("Manage system accounts"));
    }

    #[test]
    fn framework_metadata_correct() {
        let result = parse_mock();
        assert_eq!(result.framework.framework_id.as_str(), "NIST-800-53-R5");
        assert_eq!(result.framework.region, Region::Us);
        assert_eq!(result.framework.harvest_source, HarvestSource::OscalGithub);
        assert!(!result.framework.is_pivot);
    }

    #[test]
    fn oscal_harvester_object_safe() {
        let harvester = OscalHarvester::new();
        let dyn_ref: &dyn Harvester = &harvester;
        assert_eq!(dyn_ref.name(), "NIST OSCAL (GitHub)");
        assert_eq!(dyn_ref.cadence(), HarvestCadence::OnRelease);
        assert_eq!(dyn_ref.frameworks().len(), 2);
    }

    #[test]
    fn empty_catalog_produces_no_controls() {
        let json = r#"{"catalog": {"uuid": "x", "metadata": {}, "groups": []}}"#;
        let catalog: OscalCatalog = serde_json::from_str(json).unwrap();
        let result = parse_catalog(&catalog).unwrap();
        assert!(result.controls.is_empty());
    }

    #[test]
    fn idempotent_parsing() {
        let r1 = parse_mock();
        let r2 = parse_mock();
        assert_eq!(r1.controls.len(), r2.controls.len());
        for (c1, c2) in r1.controls.iter().zip(r2.controls.iter()) {
            assert_eq!(c1.control_id, c2.control_id);
            assert_eq!(c1.testing_relevant, c2.testing_relevant);
        }
    }
}
