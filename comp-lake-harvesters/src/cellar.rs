use chrono::Utc;
use serde::Deserialize;

use comp_lake_core::models::control::{Control, ControlFamily, ControlId, Severity};
use comp_lake_core::models::framework::{Framework, FrameworkId, HarvestSource, Region};

use crate::harvester::{
    HarvestCadence, HarvestConfig, HarvestError, HarvestResult, Harvester,
};

const SPARQL_ENDPOINT: &str = "https://publications.europa.eu/webapi/rdf/sparql";

/// EU legislation metadata for CELLAR harvesting.
struct EuFramework {
    id: &'static str,
    name: &'static str,
    celex: &'static str,
    version: &'static str,
}

const EU_FRAMEWORKS: &[EuFramework] = &[
    EuFramework {
        id: "DORA",
        name: "Digital Operational Resilience Act",
        celex: "32022R2554",
        version: "2022/2554",
    },
    EuFramework {
        id: "CRA",
        name: "Cyber Resilience Act",
        celex: "32024R2847",
        version: "2024/2847",
    },
    EuFramework {
        id: "NIS2",
        name: "Network and Information Security Directive",
        celex: "32022L2555",
        version: "2022/2555",
    },
    EuFramework {
        id: "GDPR",
        name: "General Data Protection Regulation",
        celex: "32016R0679",
        version: "2016/679",
    },
];

/// Harvester for EU legislation via EUR-Lex CELLAR SPARQL endpoint.
pub struct CellarHarvester {
    framework_ids: Vec<FrameworkId>,
}

impl CellarHarvester {
    #[must_use]
    pub fn new() -> Self {
        let framework_ids = EU_FRAMEWORKS
            .iter()
            .filter_map(|f| FrameworkId::new(f.id).ok())
            .collect();
        Self { framework_ids }
    }
}

impl Default for CellarHarvester {
    fn default() -> Self {
        Self::new()
    }
}

impl Harvester for CellarHarvester {
    fn name(&self) -> &'static str {
        "EUR-Lex CELLAR (SPARQL)"
    }

    fn frameworks(&self) -> &[FrameworkId] {
        &self.framework_ids
    }

    fn cadence(&self) -> HarvestCadence {
        HarvestCadence::Monthly
    }

    fn harvest<'a>(
        &'a self,
        config: &'a HarvestConfig,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<HarvestResult, HarvestError>> + Send + 'a>,
    > {
        Box::pin(harvest_cellar(config))
    }
}

async fn harvest_cellar(config: &HarvestConfig) -> Result<HarvestResult, HarvestError> {
    let eu_fw = &EU_FRAMEWORKS[0]; // DORA as primary
    let query = build_sparql_query(eu_fw.celex);

    let response = config
        .http_client
        .get(SPARQL_ENDPOINT)
        .query(&[("query", &query), ("format", &"application/json".to_owned())])
        .timeout(config.timeout)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(HarvestError::Api {
            status: response.status().as_u16(),
            message: response.text().await.unwrap_or_default(),
        });
    }

    let body: SparqlResponse = response
        .json()
        .await
        .map_err(|e| HarvestError::Parse(e.to_string()))?;

    let framework_id = FrameworkId::new(eu_fw.id).map_err(|e| HarvestError::Parse(e.to_string()))?;

    let framework = Framework::builder(framework_id.clone(), eu_fw.name)
        .version(eu_fw.version)
        .region(Region::Eu)
        .authority("European Parliament")
        .is_pivot(eu_fw.id == "DORA")
        .celex_id(eu_fw.celex)
        .harvest_source(HarvestSource::CellarSparql)
        .last_harvested(Utc::now())
        .build();

    let controls = parse_sparql_controls(&framework_id, &body);

    Ok(HarvestResult {
        framework,
        controls,
        mappings: Vec::new(),
        snapshot_version: format!("cellar-{}", Utc::now().format("%Y%m%d")),
        harvested_at: Utc::now(),
    })
}

fn build_sparql_query(celex_id: &str) -> String {
    format!(
        r#"
PREFIX cdm: <http://publications.europa.eu/ontology/cdm#>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>

SELECT DISTINCT ?article ?articleNumber ?title
WHERE {{
    ?act cdm:resource_legal_id_celex "{celex_id}"^^xsd:string .
    ?article cdm:complex_work_has_part_work ?act .
    ?article cdm:resource_legal_type <http://publications.europa.eu/resource/authority/resource-type/ARTICLE> .
    OPTIONAL {{ ?article cdm:resource_legal_article_number ?articleNumber . }}
    OPTIONAL {{ ?article cdm:expression_title ?title . FILTER(LANG(?title) = "en") }}
}}
ORDER BY ?articleNumber
"#
    )
}

fn parse_sparql_controls(framework_id: &FrameworkId, response: &SparqlResponse) -> Vec<Control> {
    response
        .results
        .bindings
        .iter()
        .filter_map(|binding| {
            let article_num = binding
                .get("articleNumber")
                .map(|v| v.value.clone())
                .unwrap_or_default();

            if article_num.is_empty() {
                return None;
            }

            let title = binding
                .get("title")
                .map_or_else(|| format!("Article {article_num}"), |v| v.value.clone());

            let control_id_str = format!(
                "{}-ART-{}",
                framework_id.as_str(),
                article_num.replace(' ', "-")
            );

            let control_id = ControlId::new(&control_id_str).ok()?;
            let testing_relevant = is_testing_relevant(framework_id.as_str(), &article_num);
            let severity = classify_severity(framework_id.as_str(), &article_num);

            Some(
                Control::builder(control_id, framework_id.clone(), &title)
                    .article_ref(format!("Art. {article_num}"))
                    .severity(severity)
                    .family(ControlFamily::new(classify_family(
                        framework_id.as_str(),
                        &article_num,
                    )))
                    .testing_relevant(testing_relevant)
                    .build(),
            )
        })
        .collect()
}

/// Classify whether a DORA/CRA/NIS2/GDPR article is testing-relevant.
fn is_testing_relevant(framework: &str, article_num: &str) -> bool {
    match framework {
        "DORA" => matches!(
            article_num,
            "24" | "25" | "26" | "27" | "28" | "29" | "30"
                | "9" | "10" | "11" | "12" | "13" | "14" | "15" | "16" | "17"
                | "19" | "20" | "21"
        ),
        "NIS2" => matches!(article_num, "21" | "23" | "24" | "25" | "26" | "29" | "32"),
        "CRA" => matches!(
            article_num,
            "10" | "11" | "12" | "13" | "14" | "15" | "16" | "17" | "18" | "20" | "21"
        ),
        "GDPR" => matches!(article_num, "32" | "35"),
        _ => false,
    }
}

/// Classify severity based on framework and article criticality.
fn classify_severity(framework: &str, article_num: &str) -> Severity {
    match framework {
        "DORA" => match article_num {
            "9" | "10" | "11" | "19" | "20" | "21" | "25" | "26" | "27" => Severity::High,
            _ => Severity::Moderate,
        },
        "NIS2" => match article_num {
            "21" | "23" => Severity::High,
            _ => Severity::Moderate,
        },
        "CRA" => match article_num {
            "10" | "11" => Severity::High,
            _ => Severity::Moderate,
        },
        "GDPR" => Severity::High, // Art 32, 35 are always high
        _ => Severity::Moderate,
    }
}

/// Classify control family from framework and article.
fn classify_family(framework: &str, article_num: &str) -> String {
    match framework {
        "DORA" => match article_num {
            "24" | "25" | "26" | "27" => "Resilience Testing",
            "9" | "10" | "11" | "12" | "13" => "Risk Management",
            "14" | "15" | "16" | "17" => "ICT Third-Party Risk",
            "19" | "20" | "21" => "Incident Management",
            "28" | "29" | "30" => "Information Sharing",
            _ => "General",
        },
        "NIS2" => match article_num {
            "21" => "Cybersecurity Risk Management",
            "23" | "24" | "25" => "Incident Reporting",
            _ => "General",
        },
        _ => "General",
    }
    .to_owned()
}

/// SPARQL JSON response format.
#[derive(Debug, Deserialize)]
struct SparqlResponse {
    results: SparqlResults,
}

#[derive(Debug, Deserialize)]
struct SparqlResults {
    bindings: Vec<std::collections::HashMap<String, SparqlValue>>,
}

#[derive(Debug, Deserialize)]
struct SparqlValue {
    value: String,
}

/// Build a `HarvestResult` from a static SPARQL-like response (for testing).
#[cfg(test)]
pub(crate) fn harvest_from_json(json: &str) -> Result<HarvestResult, HarvestError> {
    let response: SparqlResponse =
        serde_json::from_str(json).map_err(|e| HarvestError::Parse(e.to_string()))?;

    let framework_id = FrameworkId::new("DORA").map_err(|e| HarvestError::Parse(e.to_string()))?;

    let framework = Framework::builder(framework_id.clone(), "DORA")
        .version("2022/2554")
        .region(Region::Eu)
        .authority("European Parliament")
        .is_pivot(true)
        .harvest_source(HarvestSource::CellarSparql)
        .build();

    let controls = parse_sparql_controls(&framework_id, &response);

    Ok(HarvestResult {
        framework,
        controls,
        mappings: Vec::new(),
        snapshot_version: "test".to_owned(),
        harvested_at: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCK_SPARQL_RESPONSE: &str = r#"{
        "results": {
            "bindings": [
                {
                    "article": {"type": "uri", "value": "http://example.com/art24"},
                    "articleNumber": {"type": "literal", "value": "24"},
                    "title": {"type": "literal", "value": "General requirements for ICT testing"}
                },
                {
                    "article": {"type": "uri", "value": "http://example.com/art25"},
                    "articleNumber": {"type": "literal", "value": "25"},
                    "title": {"type": "literal", "value": "Testing of ICT tools and systems"}
                },
                {
                    "article": {"type": "uri", "value": "http://example.com/art26"},
                    "articleNumber": {"type": "literal", "value": "26"},
                    "title": {"type": "literal", "value": "Advanced testing of ICT tools"}
                },
                {
                    "article": {"type": "uri", "value": "http://example.com/art27"},
                    "articleNumber": {"type": "literal", "value": "27"},
                    "title": {"type": "literal", "value": "Requirements for testers"}
                },
                {
                    "article": {"type": "uri", "value": "http://example.com/art9"},
                    "articleNumber": {"type": "literal", "value": "9"},
                    "title": {"type": "literal", "value": "ICT risk management framework"}
                },
                {
                    "article": {"type": "uri", "value": "http://example.com/art1"},
                    "articleNumber": {"type": "literal", "value": "1"},
                    "title": {"type": "literal", "value": "Subject matter"}
                }
            ]
        }
    }"#;

    #[test]
    fn parse_mock_response_produces_controls() {
        let result = harvest_from_json(MOCK_SPARQL_RESPONSE).unwrap();
        assert_eq!(result.controls.len(), 6);
    }

    #[test]
    fn testing_relevant_filtered_correctly() {
        let result = harvest_from_json(MOCK_SPARQL_RESPONSE).unwrap();
        let relevant: Vec<_> = result
            .controls
            .iter()
            .filter(|c| c.testing_relevant)
            .collect();
        // Art 24, 25, 26, 27, 9 are testing-relevant; Art 1 is not
        assert_eq!(relevant.len(), 5);
    }

    #[test]
    fn control_ids_formatted_correctly() {
        let result = harvest_from_json(MOCK_SPARQL_RESPONSE).unwrap();
        let ids: Vec<_> = result
            .controls
            .iter()
            .map(|c| c.control_id.as_str().to_owned())
            .collect();
        assert!(ids.contains(&"DORA-ART-25".to_owned()));
        assert!(ids.contains(&"DORA-ART-9".to_owned()));
    }

    #[test]
    fn severity_classified_correctly() {
        let result = harvest_from_json(MOCK_SPARQL_RESPONSE).unwrap();
        let art25 = result
            .controls
            .iter()
            .find(|c| c.control_id.as_str() == "DORA-ART-25")
            .unwrap();
        assert_eq!(art25.severity, Severity::High);

        let art1 = result
            .controls
            .iter()
            .find(|c| c.control_id.as_str() == "DORA-ART-1")
            .unwrap();
        assert_eq!(art1.severity, Severity::Moderate);
    }

    #[test]
    fn family_classified_correctly() {
        let result = harvest_from_json(MOCK_SPARQL_RESPONSE).unwrap();
        let art25 = result
            .controls
            .iter()
            .find(|c| c.control_id.as_str() == "DORA-ART-25")
            .unwrap();
        assert_eq!(
            art25.family.as_ref().unwrap().as_str(),
            "Resilience Testing"
        );

        let art9 = result
            .controls
            .iter()
            .find(|c| c.control_id.as_str() == "DORA-ART-9")
            .unwrap();
        assert_eq!(art9.family.as_ref().unwrap().as_str(), "Risk Management");
    }

    #[test]
    fn framework_metadata_correct() {
        let result = harvest_from_json(MOCK_SPARQL_RESPONSE).unwrap();
        assert_eq!(result.framework.framework_id.as_str(), "DORA");
        assert!(result.framework.is_pivot);
        assert_eq!(result.framework.region, Region::Eu);
        assert_eq!(result.framework.harvest_source, HarvestSource::CellarSparql);
    }

    #[test]
    fn cellar_harvester_is_object_safe() {
        let harvester = CellarHarvester::new();
        let dyn_ref: &dyn Harvester = &harvester;
        assert_eq!(dyn_ref.name(), "EUR-Lex CELLAR (SPARQL)");
        assert_eq!(dyn_ref.cadence(), HarvestCadence::Monthly);
        assert_eq!(dyn_ref.frameworks().len(), 4);
    }

    #[test]
    fn sparql_query_contains_celex() {
        let query = build_sparql_query("32022R2554");
        assert!(query.contains("32022R2554"));
        assert!(query.contains("ARTICLE"));
    }

    #[test]
    fn empty_response_produces_no_controls() {
        let json = r#"{"results": {"bindings": []}}"#;
        let result = harvest_from_json(json).unwrap();
        assert!(result.controls.is_empty());
    }

    #[test]
    fn idempotent_parsing() {
        let r1 = harvest_from_json(MOCK_SPARQL_RESPONSE).unwrap();
        let r2 = harvest_from_json(MOCK_SPARQL_RESPONSE).unwrap();
        assert_eq!(r1.controls.len(), r2.controls.len());
        for (c1, c2) in r1.controls.iter().zip(r2.controls.iter()) {
            assert_eq!(c1.control_id, c2.control_id);
            assert_eq!(c1.testing_relevant, c2.testing_relevant);
            assert_eq!(c1.severity, c2.severity);
        }
    }
}
