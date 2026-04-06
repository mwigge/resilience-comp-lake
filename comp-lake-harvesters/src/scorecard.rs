use chrono::Utc;
use serde::Deserialize;

use comp_lake_core::models::framework::FrameworkId;

use crate::harvester::{
    HarvestCadence, HarvestConfig, HarvestError, HarvestResult, Harvester,
};

const SCORECARD_API_URL: &str = "https://api.securityscorecards.dev";

/// Harvester for `OpenSSF` Scorecard security scores.
pub struct ScorecardHarvester {
    framework_ids: Vec<FrameworkId>,
    repos: Vec<String>,
}

impl ScorecardHarvester {
    /// # Panics
    ///
    /// Panics if the hardcoded framework ID is invalid (should never happen).
    #[must_use]
    pub fn new(repos: Vec<String>) -> Self {
        Self {
            framework_ids: vec![FrameworkId::new("OSSF-SCORECARD").expect("valid framework ID")],
            repos,
        }
    }
}

impl Harvester for ScorecardHarvester {
    fn name(&self) -> &'static str {
        "OpenSSF Scorecard"
    }

    fn frameworks(&self) -> &[FrameworkId] {
        &self.framework_ids
    }

    fn cadence(&self) -> HarvestCadence {
        HarvestCadence::Weekly
    }

    fn harvest<'a>(
        &'a self,
        config: &'a HarvestConfig,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<HarvestResult, HarvestError>> + Send + 'a>,
    > {
        Box::pin(harvest_scorecard(config, &self.repos))
    }
}

async fn harvest_scorecard(
    config: &HarvestConfig,
    repos: &[String],
) -> Result<HarvestResult, HarvestError> {
    let mut all_results = Vec::new();

    for repo in repos {
        match fetch_scorecard(config, repo).await {
            Ok(result) => all_results.push(result),
            Err(HarvestError::Api { status: 404, .. }) => {
                tracing::warn!(repo = %repo, "no scorecard available, skipping");
            }
            Err(e) => return Err(e),
        }
    }

    let framework_id = FrameworkId::new("OSSF-SCORECARD").unwrap();
    let framework = comp_lake_core::models::framework::Framework::builder(
        framework_id,
        "OpenSSF Scorecard",
    )
    .version("4.0")
    .region(comp_lake_core::models::framework::Region::Global)
    .authority("OpenSSF")
    .harvest_source(comp_lake_core::models::framework::HarvestSource::ScorecardApi)
    .last_harvested(Utc::now())
    .build();

    Ok(HarvestResult {
        framework,
        controls: Vec::new(), // Scorecard produces evidence, not controls
        mappings: Vec::new(),
        snapshot_version: format!(
            "scorecard-{}-repos-{}",
            Utc::now().format("%Y%m%d"),
            all_results.len()
        ),
        harvested_at: Utc::now(),
    })
}

async fn fetch_scorecard(
    config: &HarvestConfig,
    repo: &str,
) -> Result<ScorecardResult, HarvestError> {
    let url = format!("{SCORECARD_API_URL}/projects/github.com/{repo}");

    let response = config
        .http_client
        .get(&url)
        .timeout(config.timeout)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(HarvestError::Api {
            status: response.status().as_u16(),
            message: response.text().await.unwrap_or_default(),
        });
    }

    response
        .json::<ScorecardResult>()
        .await
        .map_err(|e| HarvestError::Parse(e.to_string()))
}

/// Maps scorecard check names to relevant compliance control IDs.
#[must_use]
pub fn check_to_control_mapping(check_name: &str) -> Option<&'static [&'static str]> {
    match check_name {
        "Code-Review" | "SAST" => Some(&["NIST-800-53-R5:SA-11", "DORA-ART-25"]),
        "Branch-Protection" => Some(&["NIST-800-53-R5:CM-3", "DORA-ART-9"]),
        "Vulnerabilities" => Some(&["NIST-800-53-R5:RA-5", "DORA-ART-24"]),
        "Dependency-Update-Tool" => Some(&["NIST-800-53-R5:SI-2", "DORA-ART-10"]),
        "Signed-Releases" | "Pinned-Dependencies" => Some(&["NIST-800-53-R5:SA-12"]),
        "Binary-Artifacts" => Some(&["NIST-800-53-R5:CM-7"]),
        "Fuzzing" => Some(&["NIST-800-53-R5:SA-11", "DORA-ART-26"]),
        "Security-Policy" => Some(&["NIST-800-53-R5:PL-1"]),
        "Token-Permissions" => Some(&["NIST-800-53-R5:AC-6"]),
        "Dangerous-Workflow" | "CII-Best-Practices" => Some(&["NIST-800-53-R5:SA-11"]),
        "License" => Some(&["NIST-800-53-R5:SA-4"]),
        _ => None,
    }
}

/// Normalise a scorecard check score (0-10) to 0.0-1.0.
#[must_use]
pub fn normalise_score(raw_score: f64) -> f64 {
    (raw_score / 10.0).clamp(0.0, 1.0)
}

// ── Scorecard API response types ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct ScorecardResult {
    pub repo: ScorecardRepo,
    pub score: f64,
    #[serde(default)]
    pub checks: Vec<ScorecardCheck>,
    pub date: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct ScorecardRepo {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct ScorecardCheck {
    pub name: String,
    pub score: i32,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub documentation: Option<ScorecardDocumentation>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct ScorecardDocumentation {
    pub url: String,
}

/// Parse a scorecard response from JSON (for testing).
#[cfg(test)]
fn parse_scorecard_response(json: &str) -> Result<ScorecardResult, HarvestError> {
    serde_json::from_str(json).map_err(|e| HarvestError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCK_SCORECARD: &str = r#"{
        "date": "2024-03-15",
        "repo": {"name": "github.com/test/repo"},
        "score": 7.5,
        "checks": [
            {
                "name": "Code-Review",
                "score": 9,
                "reason": "Found 28/30 approved changesets",
                "documentation": {"url": "https://github.com/ossf/scorecard/blob/main/docs/checks.md#code-review"}
            },
            {
                "name": "Vulnerabilities",
                "score": 10,
                "reason": "0 existing vulnerabilities detected"
            },
            {
                "name": "Branch-Protection",
                "score": 6,
                "reason": "Branch protection enabled but incomplete"
            },
            {
                "name": "SAST",
                "score": 0,
                "reason": "No SAST tool detected"
            },
            {
                "name": "Fuzzing",
                "score": -1,
                "reason": "Not applicable"
            }
        ]
    }"#;

    #[test]
    fn parse_mock_scorecard() {
        let result = parse_scorecard_response(MOCK_SCORECARD).unwrap();
        assert_eq!(result.repo.name, "github.com/test/repo");
        assert!((result.score - 7.5).abs() < f64::EPSILON);
        assert_eq!(result.checks.len(), 5);
    }

    #[test]
    fn check_scores_extracted() {
        let result = parse_scorecard_response(MOCK_SCORECARD).unwrap();
        let code_review = result.checks.iter().find(|c| c.name == "Code-Review").unwrap();
        assert_eq!(code_review.score, 9);
    }

    #[test]
    fn normalise_score_range() {
        assert!((normalise_score(10.0) - 1.0).abs() < f64::EPSILON);
        assert!((normalise_score(5.0) - 0.5).abs() < f64::EPSILON);
        assert!((normalise_score(0.0) - 0.0).abs() < f64::EPSILON);
        assert!((normalise_score(-1.0) - 0.0).abs() < f64::EPSILON); // clamped
        assert!((normalise_score(15.0) - 1.0).abs() < f64::EPSILON); // clamped
    }

    #[test]
    fn check_to_control_mapping_coverage() {
        // At least 10 of 18 checks must have mappings
        let checks = [
            "Code-Review",
            "Branch-Protection",
            "Vulnerabilities",
            "Dependency-Update-Tool",
            "Signed-Releases",
            "Pinned-Dependencies",
            "SAST",
            "Binary-Artifacts",
            "Fuzzing",
            "Security-Policy",
            "Token-Permissions",
            "Dangerous-Workflow",
            "License",
            "CII-Best-Practices",
            "Maintained",
            "Packaging",
            "Contributors",
            "Webhooks",
        ];

        let mapped_count = checks
            .iter()
            .filter(|c| check_to_control_mapping(c).is_some())
            .count();
        assert!(mapped_count >= 10, "only {mapped_count} checks mapped, need >= 10");
    }

    #[test]
    fn code_review_maps_to_nist_and_dora() {
        let controls = check_to_control_mapping("Code-Review").unwrap();
        assert!(controls.contains(&"NIST-800-53-R5:SA-11"));
        assert!(controls.contains(&"DORA-ART-25"));
    }

    #[test]
    fn unknown_check_returns_none() {
        assert!(check_to_control_mapping("Unknown-Check").is_none());
    }

    #[test]
    fn scorecard_harvester_object_safe() {
        let harvester = ScorecardHarvester::new(vec!["test/repo".to_owned()]);
        let dyn_ref: &dyn Harvester = &harvester;
        assert_eq!(dyn_ref.name(), "OpenSSF Scorecard");
        assert_eq!(dyn_ref.cadence(), HarvestCadence::Weekly);
    }

    #[test]
    fn documentation_url_extracted() {
        let result = parse_scorecard_response(MOCK_SCORECARD).unwrap();
        let code_review = result.checks.iter().find(|c| c.name == "Code-Review").unwrap();
        assert!(code_review.documentation.is_some());
        assert!(code_review
            .documentation
            .as_ref()
            .unwrap()
            .url
            .contains("code-review"));
    }

    #[test]
    fn empty_checks_parses() {
        let json = r#"{"date": "2024-01-01", "repo": {"name": "test"}, "score": 0.0, "checks": []}"#;
        let result = parse_scorecard_response(json).unwrap();
        assert!(result.checks.is_empty());
    }
}
