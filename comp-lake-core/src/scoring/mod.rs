pub mod badges;
pub mod cross_framework;
pub mod engine;
pub mod rollup;
pub mod trend;

use serde::{Deserialize, Serialize};

use crate::models::framework::FrameworkId;
use badges::BadgeTier;

/// Compliance score for a single entity against a single framework.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComplianceScore {
    pub framework_id: FrameworkId,
    pub score: f64,
    pub controls_total: usize,
    pub controls_covered: usize,
    pub controls_passing: usize,
    pub controls_stale: usize,
    pub badge: BadgeTier,
}

impl ComplianceScore {
    #[must_use]
    pub fn new(
        framework_id: FrameworkId,
        controls_total: usize,
        controls_covered: usize,
        controls_passing: usize,
        controls_stale: usize,
    ) -> Self {
        let score = if controls_total == 0 {
            0.0
        } else {
            #[allow(clippy::cast_precision_loss)] // counts are always small
            let s = (controls_passing as f64 / controls_total as f64) * 100.0;
            s.min(100.0)
        };
        let badge = BadgeTier::from_score(score);

        Self {
            framework_id,
            score,
            controls_total,
            controls_covered,
            controls_passing,
            controls_stale,
            badge,
        }
    }
}
