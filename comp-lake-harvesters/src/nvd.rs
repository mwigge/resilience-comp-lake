use chrono::{DateTime, Utc};
use serde::Deserialize;

use comp_lake_core::models::framework::FrameworkId;

use crate::harvester::{
    HarvestCadence, HarvestConfig, HarvestError, HarvestResult, Harvester,
};

const NVD_API_URL: &str = "https://services.nvd.nist.gov/rest/json/cves/2.0";
const PAGE_SIZE: u32 = 2000;

/// Harvester for NIST NVD CVE data.
pub struct NvdHarvester {
    framework_ids: Vec<FrameworkId>,
}

impl NvdHarvester {
    /// # Panics
    ///
    /// Panics if the hardcoded framework ID is invalid (should never happen).
    #[must_use]
    pub fn new() -> Self {
        Self {
            framework_ids: vec![FrameworkId::new("NVD").expect("valid framework ID")],
        }
    }
}

impl Default for NvdHarvester {
    fn default() -> Self {
        Self::new()
    }
}

impl Harvester for NvdHarvester {
    fn name(&self) -> &'static str {
        "NIST NVD (CVE API 2.0)"
    }

    fn frameworks(&self) -> &[FrameworkId] {
        &self.framework_ids
    }

    fn cadence(&self) -> HarvestCadence {
        HarvestCadence::Daily
    }

    fn harvest<'a>(
        &'a self,
        config: &'a HarvestConfig,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<HarvestResult, HarvestError>> + Send + 'a>,
    > {
        Box::pin(harvest_nvd(config, None))
    }
}

/// Fetch CVEs from NVD, optionally only those modified since `last_modified`.
async fn harvest_nvd(
    config: &HarvestConfig,
    last_modified: Option<DateTime<Utc>>,
) -> Result<HarvestResult, HarvestError> {
    let mut all_cves = Vec::new();
    let mut start_index: u32 = 0;

    loop {
        let page = fetch_page(config, start_index, last_modified).await?;
        let total = page.total_results;
        all_cves.extend(page.vulnerabilities);

        #[allow(clippy::cast_possible_truncation)]
        let fetched = all_cves.len() as u32;
        if fetched >= total {
            break;
        }
        start_index += PAGE_SIZE;
    }

    let framework_id = FrameworkId::new("NVD")
        .map_err(|e| HarvestError::Parse(e.to_string()))?;
    let framework = comp_lake_core::models::framework::Framework::builder(
        framework_id,
        "NIST National Vulnerability Database",
    )
    .version("2.0")
    .region(comp_lake_core::models::framework::Region::Global)
    .authority("NIST")
    .harvest_source(comp_lake_core::models::framework::HarvestSource::NvdApi)
    .last_harvested(Utc::now())
    .build();

    Ok(HarvestResult {
        framework,
        controls: Vec::new(), // NVD produces evidence, not controls
        mappings: Vec::new(),
        snapshot_version: format!("nvd-{}-cves-{}", Utc::now().format("%Y%m%d"), all_cves.len()),
        harvested_at: Utc::now(),
    })
}

async fn fetch_page(
    config: &HarvestConfig,
    start_index: u32,
    last_modified: Option<DateTime<Utc>>,
) -> Result<NvdResponse, HarvestError> {
    let mut request = config
        .http_client
        .get(NVD_API_URL)
        .query(&[
            ("resultsPerPage", PAGE_SIZE.to_string()),
            ("startIndex", start_index.to_string()),
        ])
        .timeout(config.timeout);

    if let Some(since) = last_modified {
        let since_str = since.format("%Y-%m-%dT%H:%M:%S.000").to_string();
        let now_str = Utc::now().format("%Y-%m-%dT%H:%M:%S.000").to_string();
        request = request.query(&[
            ("lastModStartDate", since_str),
            ("lastModEndDate", now_str),
        ]);
    }

    if let Some(ref api_key) = config.api_keys.nvd_api_key {
        request = request.header("apiKey", api_key);
    }

    let response = request.send().await?;

    if response.status().as_u16() == 429 {
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        return Err(HarvestError::RateLimited {
            retry_after_secs: retry_after,
        });
    }

    if !response.status().is_success() {
        return Err(HarvestError::Api {
            status: response.status().as_u16(),
            message: response.text().await.unwrap_or_default(),
        });
    }

    response
        .json::<NvdResponse>()
        .await
        .map_err(|e| HarvestError::Parse(e.to_string()))
}

// ── NVD API response types ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct NvdResponse {
    pub total_results: u32,
    #[serde(default)]
    pub vulnerabilities: Vec<NvdVulnerability>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct NvdVulnerability {
    pub cve: NvdCve,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct NvdCve {
    pub id: String,
    pub published: String,
    pub last_modified: String,
    #[serde(default)]
    pub descriptions: Vec<NvdDescription>,
    #[serde(default)]
    pub metrics: Option<NvdMetrics>,
    #[serde(default)]
    pub weaknesses: Vec<NvdWeakness>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct NvdDescription {
    pub lang: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct NvdMetrics {
    #[serde(default)]
    pub cvss_metric_v31: Vec<NvdCvssV31>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct NvdCvssV31 {
    pub cvss_data: NvdCvssData,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct NvdCvssData {
    pub base_score: f64,
    pub base_severity: String,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // fields used in tests and future harvest
pub(crate) struct NvdWeakness {
    #[serde(default)]
    pub description: Vec<NvdDescription>,
}

/// Parse CVE data from a mock NVD response (for testing).
#[cfg(test)]
fn parse_nvd_response(json: &str) -> Result<NvdResponse, HarvestError> {
    serde_json::from_str(json).map_err(|e| HarvestError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCK_NVD_RESPONSE: &str = r#"{
        "resultsPerPage": 2,
        "startIndex": 0,
        "totalResults": 2,
        "vulnerabilities": [
            {
                "cve": {
                    "id": "CVE-2024-0001",
                    "published": "2024-01-15T00:00:00.000",
                    "lastModified": "2024-01-16T00:00:00.000",
                    "descriptions": [
                        {"lang": "en", "value": "Test vulnerability one"}
                    ],
                    "metrics": {
                        "cvssMetricV31": [
                            {
                                "cvssData": {
                                    "baseScore": 9.8,
                                    "baseSeverity": "CRITICAL"
                                }
                            }
                        ]
                    },
                    "weaknesses": [
                        {
                            "description": [
                                {"lang": "en", "value": "CWE-89"}
                            ]
                        }
                    ]
                }
            },
            {
                "cve": {
                    "id": "CVE-2024-0002",
                    "published": "2024-02-01T00:00:00.000",
                    "lastModified": "2024-02-02T00:00:00.000",
                    "descriptions": [
                        {"lang": "en", "value": "Test vulnerability two"}
                    ],
                    "metrics": {
                        "cvssMetricV31": [
                            {
                                "cvssData": {
                                    "baseScore": 5.3,
                                    "baseSeverity": "MEDIUM"
                                }
                            }
                        ]
                    },
                    "weaknesses": []
                }
            }
        ]
    }"#;

    #[test]
    fn parse_mock_response() {
        let response = parse_nvd_response(MOCK_NVD_RESPONSE).unwrap();
        assert_eq!(response.total_results, 2);
        assert_eq!(response.vulnerabilities.len(), 2);
    }

    #[test]
    fn cve_ids_extracted() {
        let response = parse_nvd_response(MOCK_NVD_RESPONSE).unwrap();
        let ids: Vec<_> = response.vulnerabilities.iter().map(|v| &v.cve.id).collect();
        assert_eq!(ids, vec!["CVE-2024-0001", "CVE-2024-0002"]);
    }

    #[test]
    fn cvss_scores_extracted() {
        let response = parse_nvd_response(MOCK_NVD_RESPONSE).unwrap();
        let cve1 = &response.vulnerabilities[0].cve;
        let score = cve1
            .metrics
            .as_ref()
            .unwrap()
            .cvss_metric_v31[0]
            .cvss_data
            .base_score;
        assert!((score - 9.8).abs() < f64::EPSILON);
    }

    #[test]
    fn cwe_ids_extracted() {
        let response = parse_nvd_response(MOCK_NVD_RESPONSE).unwrap();
        let cve1 = &response.vulnerabilities[0].cve;
        let cwe = &cve1.weaknesses[0].description[0].value;
        assert_eq!(cwe, "CWE-89");
    }

    #[test]
    fn empty_response_parses() {
        let json = r#"{"resultsPerPage": 0, "startIndex": 0, "totalResults": 0, "vulnerabilities": []}"#;
        let response = parse_nvd_response(json).unwrap();
        assert_eq!(response.total_results, 0);
        assert!(response.vulnerabilities.is_empty());
    }

    #[test]
    fn nvd_harvester_object_safe() {
        let harvester = NvdHarvester::new();
        let dyn_ref: &dyn Harvester = &harvester;
        assert_eq!(dyn_ref.name(), "NIST NVD (CVE API 2.0)");
        assert_eq!(dyn_ref.cadence(), HarvestCadence::Daily);
        assert_eq!(dyn_ref.frameworks().len(), 1);
    }

    #[test]
    fn api_key_loaded_from_config() {
        let mut config = HarvestConfig::new(std::time::Duration::from_secs(30));
        config.api_keys.nvd_api_key = Some("test-key".to_owned());
        assert!(config.api_keys.nvd_api_key.is_some());
    }
}
