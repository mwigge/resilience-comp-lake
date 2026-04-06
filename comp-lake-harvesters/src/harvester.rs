use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use comp_lake_core::models::control::Control;
use comp_lake_core::models::framework::{Framework, FrameworkId};
use comp_lake_core::models::mapping::ControlMapping;

/// How often a harvester should run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HarvestCadence {
    Daily,
    Weekly,
    Monthly,
    OnRelease,
    OnVersion,
}

/// Configuration for a harvest run.
#[derive(Debug, Clone)]
pub struct HarvestConfig {
    pub http_client: reqwest::Client,
    pub timeout: Duration,
    pub api_keys: ApiKeys,
    pub output_dir: Option<PathBuf>,
}

impl HarvestConfig {
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            timeout,
            api_keys: ApiKeys::default(),
            output_dir: None,
        }
    }
}

/// API keys loaded from environment variables.
#[derive(Clone, Default)]
pub struct ApiKeys {
    pub nvd_api_key: Option<String>,
}

impl std::fmt::Debug for ApiKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKeys")
            .field(
                "nvd_api_key",
                &self.nvd_api_key.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// Result of a harvest operation.
#[derive(Debug, Clone)]
pub struct HarvestResult {
    pub framework: Framework,
    pub controls: Vec<Control>,
    pub mappings: Vec<ControlMapping>,
    pub snapshot_version: String,
    pub harvested_at: DateTime<Utc>,
}

/// Harvest log entry for audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarvestLog {
    pub harvest_id: String,
    pub framework_id: FrameworkId,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: HarvestStatus,
    pub controls_added: u32,
    pub controls_updated: u32,
    pub mappings_added: u32,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HarvestStatus {
    Running,
    Completed,
    Failed,
}

/// Errors that can occur during harvesting.
#[derive(Debug, thiserror::Error)]
pub enum HarvestError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to parse response: {0}")]
    Parse(String),

    #[error("API error: {status} — {message}")]
    Api { status: u16, message: String },

    #[error("rate limited, retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    #[error("{0}")]
    Other(String),
}

/// Trait for framework data harvesters.
///
/// Each harvester fetches framework data from an authoritative source and
/// normalises it into the compliance data lake's domain types.
///
/// Uses boxed futures for object safety (`dyn Harvester`).
pub trait Harvester: Send + Sync {
    /// Human-readable name of this harvester.
    fn name(&self) -> &'static str;

    /// Which frameworks this harvester provides data for.
    fn frameworks(&self) -> &[FrameworkId];

    /// How often this harvester should be run.
    fn cadence(&self) -> HarvestCadence;

    /// Execute a harvest, returning normalised framework data.
    fn harvest<'a>(
        &'a self,
        config: &'a HarvestConfig,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<HarvestResult, HarvestError>> + Send + 'a>,
    >;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verify trait is object-safe
    fn _assert_object_safe(_: &dyn Harvester) {}

    #[test]
    fn harvest_config_default() {
        let config = HarvestConfig::new(Duration::from_secs(30));
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert!(config.api_keys.nvd_api_key.is_none());
    }

    #[test]
    fn cadence_serde_roundtrip() {
        for cadence in [
            HarvestCadence::Daily,
            HarvestCadence::Weekly,
            HarvestCadence::Monthly,
            HarvestCadence::OnRelease,
            HarvestCadence::OnVersion,
        ] {
            let json = serde_json::to_string(&cadence).unwrap();
            let deserialized: HarvestCadence = serde_json::from_str(&json).unwrap();
            assert_eq!(cadence, deserialized);
        }
    }

    #[test]
    fn harvest_status_serde_roundtrip() {
        for status in [
            HarvestStatus::Running,
            HarvestStatus::Completed,
            HarvestStatus::Failed,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let deserialized: HarvestStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }
}
