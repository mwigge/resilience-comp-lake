use std::sync::Arc;

use arrow::array::{BooleanArray, Float64Array, RecordBatch, StringArray, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};

use comp_lake_core::models::control::Control;
use comp_lake_core::models::evidence::Evidence;
use comp_lake_core::models::framework::Framework;
use comp_lake_core::scoring::ComplianceScore;

/// Arrow schema for the `frameworks` table.
#[must_use]
pub fn frameworks_schema() -> Schema {
    Schema::new(vec![
        Field::new("framework_id", DataType::Utf8, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("version", DataType::Utf8, false),
        Field::new("region", DataType::Utf8, false),
        Field::new("authority", DataType::Utf8, false),
        Field::new("is_pivot", DataType::Boolean, false),
        Field::new("effective_date", DataType::Utf8, true),
        Field::new("sunset_date", DataType::Utf8, true),
        Field::new("celex_id", DataType::Utf8, true),
        Field::new("eli_uri", DataType::Utf8, true),
        Field::new("harvest_source", DataType::Utf8, false),
        Field::new("last_harvested", DataType::Utf8, true),
    ])
}

/// Arrow schema for the `controls` table.
#[must_use]
pub fn controls_schema() -> Schema {
    Schema::new(vec![
        Field::new("control_id", DataType::Utf8, false),
        Field::new("framework_id", DataType::Utf8, false),
        Field::new("article_ref", DataType::Utf8, true),
        Field::new("chapter_ref", DataType::Utf8, true),
        Field::new("title", DataType::Utf8, false),
        Field::new("description", DataType::Utf8, true),
        Field::new("family", DataType::Utf8, true),
        Field::new("severity", DataType::Utf8, false),
        Field::new("testing_relevant", DataType::Boolean, false),
        Field::new("parent_id", DataType::Utf8, true),
    ])
}

/// Arrow schema for the `evidence` table.
#[must_use]
pub fn evidence_schema() -> Schema {
    Schema::new(vec![
        Field::new("evidence_id", DataType::Utf8, false),
        Field::new("entity_id", DataType::Utf8, false),
        Field::new("control_id", DataType::Utf8, false),
        Field::new("evidence_type", DataType::Utf8, false),
        Field::new("source_system", DataType::Utf8, false),
        Field::new("result", DataType::Utf8, false),
        Field::new("score", DataType::Float64, true),
        Field::new("metadata", DataType::Utf8, true),
        Field::new("observed_at", DataType::Utf8, false),
        Field::new("expires_at", DataType::Utf8, false),
    ])
}

/// Arrow schema for compliance scores.
#[must_use]
pub fn scores_schema() -> Schema {
    Schema::new(vec![
        Field::new("framework_id", DataType::Utf8, false),
        Field::new("score", DataType::Float64, false),
        Field::new("controls_total", DataType::UInt64, false),
        Field::new("controls_covered", DataType::UInt64, false),
        Field::new("controls_passing", DataType::UInt64, false),
        Field::new("controls_stale", DataType::UInt64, false),
        Field::new("badge", DataType::Utf8, false),
    ])
}

/// Convert frameworks to an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an Arrow error if batch construction fails.
pub fn frameworks_to_record_batch(frameworks: &[Framework]) -> arrow::error::Result<RecordBatch> {
    let schema = Arc::new(frameworks_schema());
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from_iter_values(
                frameworks.iter().map(|f| f.framework_id.as_str()),
            )),
            Arc::new(StringArray::from_iter_values(
                frameworks.iter().map(|f| f.name.as_str()),
            )),
            Arc::new(StringArray::from_iter_values(
                frameworks.iter().map(|f| f.version.as_str()),
            )),
            Arc::new(StringArray::from_iter_values(
                frameworks.iter().map(|f| format!("{:?}", f.region)),
            )),
            Arc::new(StringArray::from_iter_values(
                frameworks.iter().map(|f| f.authority.as_str()),
            )),
            Arc::new(BooleanArray::from(
                frameworks.iter().map(|f| f.is_pivot).collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                frameworks
                    .iter()
                    .map(|f| f.effective_date.map(|d| d.format("%Y-%m-%d").to_string()))
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                frameworks
                    .iter()
                    .map(|f| f.sunset_date.map(|d| d.format("%Y-%m-%d").to_string()))
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                frameworks
                    .iter()
                    .map(|f| f.celex_id.clone())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                frameworks
                    .iter()
                    .map(|f| f.eli_uri.clone())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from_iter_values(
                frameworks.iter().map(|f| format!("{:?}", f.harvest_source)),
            )),
            Arc::new(StringArray::from(
                frameworks
                    .iter()
                    .map(|f| f.last_harvested.map(|d| d.to_rfc3339()))
                    .collect::<Vec<_>>(),
            )),
        ],
    )
}

/// Convert controls to an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an Arrow error if batch construction fails.
pub fn controls_to_record_batch(controls: &[Control]) -> arrow::error::Result<RecordBatch> {
    let schema = Arc::new(controls_schema());
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from_iter_values(
                controls.iter().map(|c| c.control_id.as_str()),
            )),
            Arc::new(StringArray::from_iter_values(
                controls.iter().map(|c| c.framework_id.as_str()),
            )),
            Arc::new(StringArray::from(
                controls
                    .iter()
                    .map(|c| c.article_ref.clone())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                controls
                    .iter()
                    .map(|c| c.chapter_ref.clone())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from_iter_values(
                controls.iter().map(|c| c.title.as_str()),
            )),
            Arc::new(StringArray::from(
                controls
                    .iter()
                    .map(|c| Some(c.description.as_str()))
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                controls
                    .iter()
                    .map(|c| c.family.as_ref().map(|f| f.as_str().to_owned()))
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from_iter_values(
                controls.iter().map(|c| format!("{:?}", c.severity)),
            )),
            Arc::new(BooleanArray::from(
                controls
                    .iter()
                    .map(|c| c.testing_relevant)
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                controls
                    .iter()
                    .map(|c| c.parent_id.as_ref().map(|p| p.as_str().to_owned()))
                    .collect::<Vec<_>>(),
            )),
        ],
    )
}

/// Convert evidence to an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an Arrow error if batch construction fails.
pub fn evidence_to_record_batch(evidence: &[Evidence]) -> arrow::error::Result<RecordBatch> {
    let schema = Arc::new(evidence_schema());
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from_iter_values(
                evidence.iter().map(|e| e.evidence_id.to_string()),
            )),
            Arc::new(StringArray::from_iter_values(
                evidence.iter().map(|e| e.entity_id.as_str().to_owned()),
            )),
            Arc::new(StringArray::from_iter_values(
                evidence.iter().map(|e| e.control_id.as_str().to_owned()),
            )),
            Arc::new(StringArray::from_iter_values(
                evidence.iter().map(|e| format!("{:?}", e.evidence_type)),
            )),
            Arc::new(StringArray::from_iter_values(
                evidence.iter().map(|e| e.source_system.as_str().to_owned()),
            )),
            Arc::new(StringArray::from_iter_values(
                evidence.iter().map(|e| format!("{:?}", e.result)),
            )),
            Arc::new(Float64Array::from(
                evidence.iter().map(|e| e.score).collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                evidence
                    .iter()
                    .map(|e| Some(e.metadata.to_string()))
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from_iter_values(
                evidence.iter().map(|e| e.observed_at.to_rfc3339()),
            )),
            Arc::new(StringArray::from_iter_values(
                evidence.iter().map(|e| e.expires_at.to_rfc3339()),
            )),
        ],
    )
}

/// Convert compliance scores to an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an Arrow error if batch construction fails.
#[allow(clippy::cast_possible_truncation)] // counts are always small
pub fn scores_to_record_batch(scores: &[ComplianceScore]) -> arrow::error::Result<RecordBatch> {
    let schema = Arc::new(scores_schema());
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from_iter_values(
                scores.iter().map(|s| s.framework_id.as_str()),
            )),
            Arc::new(Float64Array::from_iter_values(
                scores.iter().map(|s| s.score),
            )),
            Arc::new(UInt64Array::from_iter_values(
                scores.iter().map(|s| s.controls_total as u64),
            )),
            Arc::new(UInt64Array::from_iter_values(
                scores.iter().map(|s| s.controls_covered as u64),
            )),
            Arc::new(UInt64Array::from_iter_values(
                scores.iter().map(|s| s.controls_passing as u64),
            )),
            Arc::new(UInt64Array::from_iter_values(
                scores.iter().map(|s| s.controls_stale as u64),
            )),
            Arc::new(StringArray::from_iter_values(
                scores.iter().map(|s| format!("{:?}", s.badge)),
            )),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use comp_lake_core::models::control::{ControlFamily, ControlId, Severity};
    use comp_lake_core::models::evidence::{
        EvidenceId, EvidenceResult, EvidenceType, SourceSystem,
    };
    use comp_lake_core::models::framework::{FrameworkId, HarvestSource, Region};
    use comp_lake_core::models::freshness::compute_expires_at;
    use comp_lake_core::models::org::EntityId;

    #[test]
    fn frameworks_batch_valid() {
        let fw = Framework::builder(FrameworkId::new("DORA").unwrap(), "DORA")
            .version("2022/2554")
            .region(Region::Eu)
            .authority("EU/EP")
            .harvest_source(HarvestSource::CellarSparql)
            .build();
        let batch = frameworks_to_record_batch(&[fw]).unwrap();
        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 12);
    }

    #[test]
    fn controls_batch_valid() {
        let ctrl = Control::builder(
            ControlId::new("DORA-25").unwrap(),
            FrameworkId::new("DORA").unwrap(),
            "ICT incident management",
        )
        .severity(Severity::High)
        .family(ControlFamily::new("Resilience Testing"))
        .testing_relevant(true)
        .build();
        let batch = controls_to_record_batch(&[ctrl]).unwrap();
        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 10);
    }

    #[test]
    fn evidence_batch_valid() {
        let now = Utc::now();
        let et = EvidenceType::ChaosExperiment;
        let ev = Evidence {
            evidence_id: EvidenceId::new(),
            entity_id: EntityId::new("team-alpha"),
            control_id: ControlId::new("DORA-25").unwrap(),
            evidence_type: et,
            source_system: SourceSystem::new("tumult"),
            result: EvidenceResult::Pass,
            score: Some(1.0),
            metadata: serde_json::json!({}),
            observed_at: now,
            expires_at: compute_expires_at(now, &et),
        };
        let batch = evidence_to_record_batch(&[ev]).unwrap();
        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 10);
    }

    #[test]
    fn scores_batch_valid() {
        let score = ComplianceScore::new(FrameworkId::new("DORA").unwrap(), 10, 8, 7, 1);
        let batch = scores_to_record_batch(&[score]).unwrap();
        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 7);
    }

    #[test]
    fn empty_batch_valid() {
        let batch = frameworks_to_record_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 12);
    }

    #[test]
    fn schema_field_counts_match_duckdb() {
        assert_eq!(frameworks_schema().fields().len(), 12);
        assert_eq!(controls_schema().fields().len(), 10);
        assert_eq!(evidence_schema().fields().len(), 10);
        assert_eq!(scores_schema().fields().len(), 7);
    }
}
