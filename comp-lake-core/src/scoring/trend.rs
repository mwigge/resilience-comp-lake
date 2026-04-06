use serde::{Deserialize, Serialize};

use super::ComplianceScore;

/// Direction of score movement over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trend {
    Improving,
    Stable,
    Degrading,
}

/// Compute trend from current score vs score 30 days ago.
///
/// - >= +5 points: `Improving`
/// - <= -5 points: `Degrading`
/// - Otherwise: `Stable`
#[must_use]
pub fn compute_trend(current_score: f64, score_30d_ago: f64) -> Trend {
    let delta = current_score - score_30d_ago;
    if delta >= 5.0 {
        Trend::Improving
    } else if delta <= -5.0 {
        Trend::Degrading
    } else {
        Trend::Stable
    }
}

/// Returns `true` if more than 25% of evidence backing a score is expired.
#[must_use]
pub fn stale_warning(score: &ComplianceScore) -> bool {
    let total_evidence = score.controls_covered + score.controls_stale;
    if total_evidence == 0 {
        return false;
    }
    #[allow(clippy::cast_precision_loss)]
    let stale_ratio = score.controls_stale as f64 / total_evidence as f64;
    stale_ratio > 0.25
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::framework::FrameworkId;

    #[test]
    fn improving_trend() {
        assert_eq!(compute_trend(80.0, 70.0), Trend::Improving);
        assert_eq!(compute_trend(75.0, 70.0), Trend::Improving);
    }

    #[test]
    fn degrading_trend() {
        assert_eq!(compute_trend(60.0, 70.0), Trend::Degrading);
        assert_eq!(compute_trend(65.0, 70.0), Trend::Degrading);
    }

    #[test]
    fn stable_trend() {
        assert_eq!(compute_trend(72.0, 70.0), Trend::Stable);
        assert_eq!(compute_trend(70.0, 70.0), Trend::Stable);
        assert_eq!(compute_trend(66.0, 70.0), Trend::Stable);
    }

    #[test]
    fn boundary_values() {
        assert_eq!(compute_trend(75.0, 70.0), Trend::Improving); // +5 exactly
        assert_eq!(compute_trend(65.0, 70.0), Trend::Degrading); // -5 exactly
        assert_eq!(compute_trend(74.9, 70.0), Trend::Stable); // just under +5
        assert_eq!(compute_trend(65.1, 70.0), Trend::Stable); // just over -5
    }

    #[test]
    fn stale_warning_over_25_percent() {
        let score = ComplianceScore::new(
            FrameworkId::new("DORA").unwrap(),
            10, 6, 5, 3, // 3 stale out of 9 total evidence = 33%
        );
        assert!(stale_warning(&score));
    }

    #[test]
    fn no_stale_warning_under_25_percent() {
        let score = ComplianceScore::new(
            FrameworkId::new("DORA").unwrap(),
            10, 8, 7, 1, // 1 stale out of 9 total = 11%
        );
        assert!(!stale_warning(&score));
    }

    #[test]
    fn no_stale_warning_when_no_evidence() {
        let score = ComplianceScore::new(
            FrameworkId::new("DORA").unwrap(),
            10, 0, 0, 0,
        );
        assert!(!stale_warning(&score));
    }

    #[test]
    fn stale_warning_at_boundary() {
        // 1 stale out of 4 total = 25% exactly — should NOT warn (> not >=)
        let score = ComplianceScore::new(
            FrameworkId::new("DORA").unwrap(),
            10, 3, 3, 1,
        );
        assert!(!stale_warning(&score));
    }

    #[test]
    fn trend_serde_roundtrip() {
        for t in [Trend::Improving, Trend::Stable, Trend::Degrading] {
            let json = serde_json::to_string(&t).unwrap();
            let deserialized: Trend = serde_json::from_str(&json).unwrap();
            assert_eq!(t, deserialized);
        }
    }
}
