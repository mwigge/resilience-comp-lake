use serde::{Deserialize, Serialize};

use super::ComplianceScore;

/// Per-framework badge tier based on compliance score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BadgeTier {
    None,
    Bronze,
    Silver,
    Gold,
    Platinum,
}

impl BadgeTier {
    /// Determine badge tier from a compliance score (0.0–100.0).
    #[must_use]
    pub fn from_score(score: f64) -> Self {
        if score >= 95.0 {
            Self::Platinum
        } else if score >= 85.0 {
            Self::Gold
        } else if score >= 70.0 {
            Self::Silver
        } else if score >= 50.0 {
            Self::Bronze
        } else {
            Self::None
        }
    }
}

/// Cross-framework achievement badges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrossFrameworkBadge {
    /// DORA framework score >= Silver
    DoraReady,
    /// PCI DSS framework score >= Silver
    PciChampion,
    /// All EU frameworks (DORA, NIS2, CRA, GDPR) >= Silver
    EuCompliant,
    /// All frameworks >= Bronze
    FullSpectrum,
    /// All frameworks >= Gold
    ResilienceLeader,
}

/// Evaluate which cross-framework badges are earned from a set of framework scores.
#[must_use]
pub fn evaluate_cross_badges(scores: &[ComplianceScore]) -> Vec<CrossFrameworkBadge> {
    let mut badges = Vec::new();

    let find = |id: &str| scores.iter().find(|s| s.framework_id.as_str() == id);

    if let Some(dora) = find("DORA") {
        if dora.badge >= BadgeTier::Silver {
            badges.push(CrossFrameworkBadge::DoraReady);
        }
    }

    if let Some(pci) = find("PCI-DSS-4") {
        if pci.badge >= BadgeTier::Silver {
            badges.push(CrossFrameworkBadge::PciChampion);
        }
    }

    let eu_frameworks = ["DORA", "NIS2", "CRA", "GDPR"];
    let eu_all_silver = eu_frameworks
        .iter()
        .all(|id| find(id).is_some_and(|s| s.badge >= BadgeTier::Silver));
    if eu_all_silver {
        badges.push(CrossFrameworkBadge::EuCompliant);
    }

    if !scores.is_empty() && scores.iter().all(|s| s.badge >= BadgeTier::Bronze) {
        badges.push(CrossFrameworkBadge::FullSpectrum);
    }

    if !scores.is_empty() && scores.iter().all(|s| s.badge >= BadgeTier::Gold) {
        badges.push(CrossFrameworkBadge::ResilienceLeader);
    }

    badges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::framework::FrameworkId;

    #[test]
    fn badge_thresholds() {
        assert_eq!(BadgeTier::from_score(0.0), BadgeTier::None);
        assert_eq!(BadgeTier::from_score(49.9), BadgeTier::None);
        assert_eq!(BadgeTier::from_score(50.0), BadgeTier::Bronze);
        assert_eq!(BadgeTier::from_score(69.9), BadgeTier::Bronze);
        assert_eq!(BadgeTier::from_score(70.0), BadgeTier::Silver);
        assert_eq!(BadgeTier::from_score(84.9), BadgeTier::Silver);
        assert_eq!(BadgeTier::from_score(85.0), BadgeTier::Gold);
        assert_eq!(BadgeTier::from_score(94.9), BadgeTier::Gold);
        assert_eq!(BadgeTier::from_score(95.0), BadgeTier::Platinum);
        assert_eq!(BadgeTier::from_score(100.0), BadgeTier::Platinum);
    }

    fn make_score(id: &str, pct: f64) -> ComplianceScore {
        let total: usize = 100;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let passing = ((pct / 100.0) * total as f64) as usize;
        ComplianceScore::new(FrameworkId::new(id).unwrap(), total, passing, passing, 0)
    }

    #[test]
    fn cross_badge_dora_ready() {
        let scores = vec![make_score("DORA", 75.0)];
        let badges = evaluate_cross_badges(&scores);
        assert!(badges.contains(&CrossFrameworkBadge::DoraReady));
    }

    #[test]
    fn cross_badge_eu_compliant() {
        let scores = vec![
            make_score("DORA", 80.0),
            make_score("NIS2", 75.0),
            make_score("CRA", 70.0),
            make_score("GDPR", 90.0),
        ];
        let badges = evaluate_cross_badges(&scores);
        assert!(badges.contains(&CrossFrameworkBadge::EuCompliant));
    }

    #[test]
    fn cross_badge_full_spectrum() {
        let scores = vec![make_score("DORA", 55.0), make_score("NIST-800-53", 60.0)];
        let badges = evaluate_cross_badges(&scores);
        assert!(badges.contains(&CrossFrameworkBadge::FullSpectrum));
    }

    #[test]
    fn cross_badge_resilience_leader() {
        let scores = vec![make_score("DORA", 90.0), make_score("NIST-800-53", 88.0)];
        let badges = evaluate_cross_badges(&scores);
        assert!(badges.contains(&CrossFrameworkBadge::ResilienceLeader));
    }

    #[test]
    fn no_badges_on_low_scores() {
        let scores = vec![make_score("DORA", 30.0)];
        let badges = evaluate_cross_badges(&scores);
        assert!(badges.is_empty());
    }

    #[test]
    fn empty_scores_no_badges() {
        let badges = evaluate_cross_badges(&[]);
        assert!(badges.is_empty());
    }

    #[test]
    fn cross_framework_badge_is_minimum_tier() {
        // If one framework is below Bronze, FullSpectrum is NOT awarded
        // even if all others are Gold.
        let scores = vec![
            make_score("DORA", 90.0),
            make_score("NIST-800-53", 85.0),
            make_score("PCI-DSS-4", 40.0), // Below Bronze threshold
        ];
        let badges = evaluate_cross_badges(&scores);
        assert!(!badges.contains(&CrossFrameworkBadge::FullSpectrum));
        assert!(!badges.contains(&CrossFrameworkBadge::ResilienceLeader));
    }

    #[test]
    fn cross_badge_pci_champion() {
        let scores = vec![make_score("PCI-DSS-4", 75.0)];
        let badges = evaluate_cross_badges(&scores);
        assert!(badges.contains(&CrossFrameworkBadge::PciChampion));
    }

    #[test]
    fn cross_badge_pci_champion_below_silver() {
        let scores = vec![make_score("PCI-DSS-4", 55.0)];
        let badges = evaluate_cross_badges(&scores);
        assert!(!badges.contains(&CrossFrameworkBadge::PciChampion));
    }

    #[test]
    fn eu_compliant_requires_all_four() {
        // Missing CRA framework — should not award EuCompliant
        let scores = vec![
            make_score("DORA", 80.0),
            make_score("NIS2", 75.0),
            make_score("GDPR", 90.0),
        ];
        let badges = evaluate_cross_badges(&scores);
        assert!(!badges.contains(&CrossFrameworkBadge::EuCompliant));
    }

    #[test]
    fn badge_tier_ordering() {
        assert!(BadgeTier::Platinum > BadgeTier::Gold);
        assert!(BadgeTier::Gold > BadgeTier::Silver);
        assert!(BadgeTier::Silver > BadgeTier::Bronze);
        assert!(BadgeTier::Bronze > BadgeTier::None);
    }

    #[test]
    fn badge_serde_roundtrip() {
        for tier in [
            BadgeTier::None,
            BadgeTier::Bronze,
            BadgeTier::Silver,
            BadgeTier::Gold,
            BadgeTier::Platinum,
        ] {
            let json = serde_json::to_string(&tier).unwrap();
            let deserialized: BadgeTier = serde_json::from_str(&json).unwrap();
            assert_eq!(tier, deserialized);
        }
    }
}
