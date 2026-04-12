use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use comp_lake_core::models::framework::FrameworkId;

/// Generate the snapshot file path for a framework at a given time.
#[must_use]
pub fn snapshot_path(
    base_dir: &Path,
    framework_id: &FrameworkId,
    harvested_at: DateTime<Utc>,
) -> PathBuf {
    base_dir
        .join(framework_id.as_str())
        .join(format!("{}.parquet", harvested_at.format("%Y-%m-%d")))
}

/// List existing snapshots for a framework, sorted newest first.
///
/// # Errors
///
/// Returns an I/O error if the directory can't be read.
pub fn list_snapshots(
    base_dir: &Path,
    framework_id: &FrameworkId,
) -> std::io::Result<Vec<PathBuf>> {
    let dir = base_dir.join(framework_id.as_str());
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut snapshots: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "parquet"))
        .collect();

    snapshots.sort();
    snapshots.reverse(); // newest first
    Ok(snapshots)
}

/// Apply retention policy: keep only the most recent `max_snapshots`.
///
/// # Errors
///
/// Returns an I/O error if snapshot deletion fails.
pub fn apply_retention(
    base_dir: &Path,
    framework_id: &FrameworkId,
    max_snapshots: usize,
) -> std::io::Result<usize> {
    let snapshots = list_snapshots(base_dir, framework_id)?;
    let mut deleted = 0;

    for old in snapshots.into_iter().skip(max_snapshots) {
        std::fs::remove_file(&old)?;
        deleted += 1;
    }

    Ok(deleted)
}

/// Ensure the framework snapshot directory exists.
///
/// # Errors
///
/// Returns an I/O error if directory creation fails.
pub fn ensure_snapshot_dir(base_dir: &Path, framework_id: &FrameworkId) -> std::io::Result<()> {
    let dir = base_dir.join(framework_id.as_str());
    std::fs::create_dir_all(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fw() -> FrameworkId {
        FrameworkId::new("DORA").unwrap()
    }

    #[test]
    fn snapshot_path_format() {
        let base = Path::new("/data/frameworks");
        let ts = chrono::DateTime::parse_from_rfc3339("2026-03-15T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let path = snapshot_path(base, &fw(), ts);
        assert_eq!(
            path.to_str().unwrap(),
            "/data/frameworks/DORA/2026-03-15.parquet"
        );
    }

    #[test]
    fn list_snapshots_empty_dir() {
        let dir = tempdir().unwrap();
        let snaps = list_snapshots(dir.path(), &fw()).unwrap();
        assert!(snaps.is_empty());
    }

    #[test]
    fn list_snapshots_finds_parquet_files() {
        let dir = tempdir().unwrap();
        let fw_dir = dir.path().join("DORA");
        std::fs::create_dir_all(&fw_dir).unwrap();
        std::fs::write(fw_dir.join("2026-01-01.parquet"), b"test").unwrap();
        std::fs::write(fw_dir.join("2026-02-01.parquet"), b"test").unwrap();
        std::fs::write(fw_dir.join("notes.txt"), b"not a snapshot").unwrap();

        let snaps = list_snapshots(dir.path(), &fw()).unwrap();
        assert_eq!(snaps.len(), 2);
        // Newest first
        assert!(snaps[0].to_str().unwrap().contains("2026-02-01"));
    }

    #[test]
    fn retention_keeps_latest() {
        let dir = tempdir().unwrap();
        let fw_dir = dir.path().join("DORA");
        std::fs::create_dir_all(&fw_dir).unwrap();

        for month in 1..=5 {
            std::fs::write(fw_dir.join(format!("2026-{month:02}-01.parquet")), b"test").unwrap();
        }

        let deleted = apply_retention(dir.path(), &fw(), 3).unwrap();
        assert_eq!(deleted, 2);

        let remaining = list_snapshots(dir.path(), &fw()).unwrap();
        assert_eq!(remaining.len(), 3);
        // Newest 3 kept
        assert!(remaining[0].to_str().unwrap().contains("2026-05-01"));
        assert!(remaining[2].to_str().unwrap().contains("2026-03-01"));
    }

    #[test]
    fn retention_noop_under_limit() {
        let dir = tempdir().unwrap();
        let fw_dir = dir.path().join("DORA");
        std::fs::create_dir_all(&fw_dir).unwrap();
        std::fs::write(fw_dir.join("2026-01-01.parquet"), b"test").unwrap();

        let deleted = apply_retention(dir.path(), &fw(), 12).unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn ensure_snapshot_dir_creates_path() {
        let dir = tempdir().unwrap();
        ensure_snapshot_dir(dir.path(), &fw()).unwrap();
        assert!(dir.path().join("DORA").exists());
    }
}
