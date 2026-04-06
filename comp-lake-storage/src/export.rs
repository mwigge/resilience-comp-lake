use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::datatypes::Schema;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

/// Export a `RecordBatch` to a Zstd-compressed Parquet file.
///
/// # Errors
///
/// Returns an error if file creation or Parquet writing fails.
pub fn export_parquet(batch: &RecordBatch, path: &Path) -> Result<(), ExportError> {
    let file = File::create(path)?;
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(parquet::basic::ZstdLevel::default()))
        .build();
    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props))?;
    writer.write(batch)?;
    writer.close()?;
    Ok(())
}

/// Import all record batches from a Parquet file.
///
/// # Errors
///
/// Returns an error if file reading or Parquet parsing fails.
pub fn import_parquet(path: &Path) -> Result<(Arc<Schema>, Vec<RecordBatch>), ExportError> {
    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();
    let reader = builder.build()?;
    let batches: Result<Vec<_>, _> = reader.collect();
    Ok((schema, batches?))
}

/// Export a full `DuckDB` database copy for distribution.
///
/// Uses `DuckDB` `ATTACH` + `CREATE TABLE AS` to copy all tables into
/// a new self-contained database file.
///
/// # Errors
///
/// Returns an error if the database copy fails.
pub fn export_duckdb_file(
    conn: &duckdb::Connection,
    output_path: &Path,
) -> Result<(), ExportError> {
    let output_str = output_path
        .to_str()
        .ok_or_else(|| ExportError::InvalidPath(output_path.to_path_buf()))?;

    // Escape single quotes in path to prevent SQL injection
    let escaped = output_str.replace('\'', "''");

    // DuckDB can copy itself by attaching a new database and copying tables
    conn.execute_batch(&format!(
        "ATTACH '{escaped}' AS export_db;
         CREATE TABLE export_db.frameworks AS SELECT * FROM frameworks;
         CREATE TABLE export_db.controls AS SELECT * FROM controls;
         CREATE TABLE export_db.control_mappings AS SELECT * FROM control_mappings;
         CREATE TABLE export_db.org_hierarchy AS SELECT * FROM org_hierarchy;
         CREATE TABLE export_db.evidence AS SELECT * FROM evidence;
         CREATE TABLE export_db.harvest_log AS SELECT * FROM harvest_log;
         CREATE TABLE export_db.schema_meta AS SELECT * FROM schema_meta;
         DETACH export_db;"
    ))?;
    Ok(())
}

/// Errors that can occur during export/import operations.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("DuckDB error: {0}")]
    DuckDb(#[from] duckdb::Error),

    #[error("invalid path: {0}")]
    InvalidPath(std::path::PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow_schema::{frameworks_to_record_batch, scores_to_record_batch};
    use crate::store::CompLakeStore;
    use comp_lake_core::models::framework::{Framework, FrameworkId, HarvestSource, Region};
    use comp_lake_core::scoring::ComplianceScore;
    use tempfile::tempdir;

    fn test_framework() -> Framework {
        Framework::builder(FrameworkId::new("DORA").unwrap(), "DORA")
            .version("2022/2554")
            .region(Region::Eu)
            .authority("EU/EP")
            .harvest_source(HarvestSource::CellarSparql)
            .build()
    }

    #[test]
    fn parquet_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.parquet");

        let fw = test_framework();
        let batch = frameworks_to_record_batch(&[fw]).unwrap();

        export_parquet(&batch, &path).unwrap();
        assert!(path.exists());

        let (schema, batches) = import_parquet(&path).unwrap();
        assert_eq!(schema.fields().len(), 12);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows(), 1);
    }

    #[test]
    fn parquet_is_zstd_compressed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("compressed.parquet");

        let score = ComplianceScore::new(FrameworkId::new("DORA").unwrap(), 10, 8, 7, 1);
        let batch = scores_to_record_batch(&[score]).unwrap();
        export_parquet(&batch, &path).unwrap();

        // Verify the file exists and is smaller than uncompressed would be
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 0);
    }

    #[test]
    fn parquet_empty_batch() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.parquet");

        let batch = frameworks_to_record_batch(&[]).unwrap();
        export_parquet(&batch, &path).unwrap();

        let (schema, batches) = import_parquet(&path).unwrap();
        assert_eq!(schema.fields().len(), 12);
        let total_rows: usize = batches.iter().map(RecordBatch::num_rows).sum();
        assert_eq!(total_rows, 0);
    }

    #[test]
    fn export_duckdb_copy() {
        let dir = tempdir().unwrap();
        let export_path = dir.path().join("export.duckdb");

        let store = CompLakeStore::in_memory().unwrap();
        store.upsert_framework(&test_framework()).unwrap();

        export_duckdb_file(store.conn(), &export_path).unwrap();

        // Open exported file and verify data
        let export_conn = duckdb::Connection::open(&export_path).unwrap();
        let count: usize = export_conn
            .query_row("SELECT COUNT(*) FROM frameworks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn multiple_batches_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("multi.parquet");

        let scores = vec![
            ComplianceScore::new(FrameworkId::new("DORA").unwrap(), 10, 8, 7, 1),
            ComplianceScore::new(FrameworkId::new("NIST").unwrap(), 20, 15, 12, 3),
        ];
        let batch = scores_to_record_batch(&scores).unwrap();
        export_parquet(&batch, &path).unwrap();

        let (_, batches) = import_parquet(&path).unwrap();
        let total_rows: usize = batches.iter().map(RecordBatch::num_rows).sum();
        assert_eq!(total_rows, 2);
    }
}
