use std::path::PathBuf;

/// Application configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub db_path: Option<PathBuf>,
    pub seed_data_dir: PathBuf,
    pub nvd_api_key: Option<String>,
    pub listen_port: u16,
    pub snapshot_dir: PathBuf,
    pub max_snapshots: usize,
}

impl AppConfig {
    /// Load configuration from environment variables with sensible defaults.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            db_path: std::env::var("COMP_LAKE_DB_PATH").ok().map(PathBuf::from),
            seed_data_dir: std::env::var("COMP_LAKE_SEED_DIR")
                .map_or_else(|_| PathBuf::from("data/seed"), PathBuf::from),
            nvd_api_key: std::env::var("NVD_API_KEY").ok(),
            listen_port: std::env::var("COMP_LAKE_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080),
            snapshot_dir: std::env::var("COMP_LAKE_SNAPSHOT_DIR")
                .map_or_else(|_| PathBuf::from("data/frameworks"), PathBuf::from),
            max_snapshots: std::env::var("COMP_LAKE_MAX_SNAPSHOTS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(12),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let config = AppConfig {
            db_path: None,
            seed_data_dir: PathBuf::from("data/seed"),
            nvd_api_key: None,
            listen_port: 8080,
            snapshot_dir: PathBuf::from("data/frameworks"),
            max_snapshots: 12,
        };
        assert_eq!(config.listen_port, 8080);
        assert_eq!(config.max_snapshots, 12);
        assert!(config.db_path.is_none());
    }
}
