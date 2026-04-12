use chrono::{DateTime, Duration, Utc};

use super::evidence::{Evidence, EvidenceType};

/// Returns the validity period for a given evidence type.
#[must_use]
pub fn freshness_period(evidence_type: &EvidenceType) -> Duration {
    match evidence_type {
        EvidenceType::ChaosExperiment => Duration::days(90),
        EvidenceType::GameDay => Duration::days(180),
        EvidenceType::PenTest | EvidenceType::AuditFinding => Duration::days(365),
        EvidenceType::VulnScan | EvidenceType::DoraMetric | EvidenceType::UnitTest => {
            Duration::days(30)
        }
        EvidenceType::Scorecard => Duration::days(14),
        EvidenceType::IntegrationTest => Duration::days(60),
    }
}

/// Computes the expiry timestamp for evidence observed at a given time.
#[must_use]
pub fn compute_expires_at(
    observed_at: DateTime<Utc>,
    evidence_type: &EvidenceType,
) -> DateTime<Utc> {
    observed_at + freshness_period(evidence_type)
}

/// Returns `true` if the evidence has not expired at the given time.
#[must_use]
pub fn is_fresh(evidence: &Evidence, now: DateTime<Utc>) -> bool {
    now < evidence.expires_at
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::control::ControlId;
    use crate::models::evidence::{EvidenceId, EvidenceResult, SourceSystem};
    use crate::models::org::EntityId;

    fn make_evidence(evidence_type: EvidenceType, observed_at: DateTime<Utc>) -> Evidence {
        let expires_at = compute_expires_at(observed_at, &evidence_type);
        Evidence {
            evidence_id: EvidenceId::new(),
            entity_id: EntityId::new("team-test"),
            control_id: ControlId::new("CTRL-1").unwrap(),
            evidence_type,
            source_system: SourceSystem::new("test"),
            result: EvidenceResult::Pass,
            score: None,
            metadata: serde_json::Value::Null,
            observed_at,
            expires_at,
        }
    }

    #[test]
    fn freshness_periods() {
        assert_eq!(
            freshness_period(&EvidenceType::ChaosExperiment).num_days(),
            90
        );
        assert_eq!(freshness_period(&EvidenceType::GameDay).num_days(), 180);
        assert_eq!(freshness_period(&EvidenceType::PenTest).num_days(), 365);
        assert_eq!(freshness_period(&EvidenceType::VulnScan).num_days(), 30);
        assert_eq!(freshness_period(&EvidenceType::DoraMetric).num_days(), 30);
        assert_eq!(freshness_period(&EvidenceType::Scorecard).num_days(), 14);
        assert_eq!(
            freshness_period(&EvidenceType::AuditFinding).num_days(),
            365
        );
        assert_eq!(freshness_period(&EvidenceType::UnitTest).num_days(), 30);
        assert_eq!(
            freshness_period(&EvidenceType::IntegrationTest).num_days(),
            60
        );
    }

    #[test]
    fn compute_expires_at_adds_correct_duration() {
        let observed = Utc::now();
        let expires = compute_expires_at(observed, &EvidenceType::ChaosExperiment);
        assert_eq!((expires - observed).num_days(), 90);
    }

    #[test]
    fn is_fresh_within_period() {
        let observed = Utc::now();
        let ev = make_evidence(EvidenceType::VulnScan, observed);
        let within = observed + Duration::days(15);
        assert!(is_fresh(&ev, within));
    }

    #[test]
    fn is_fresh_at_boundary() {
        let observed = Utc::now();
        let ev = make_evidence(EvidenceType::VulnScan, observed);
        assert!(
            !is_fresh(&ev, ev.expires_at),
            "boundary now==expires_at should be stale"
        );
    }

    #[test]
    fn is_stale_after_period() {
        let observed = Utc::now();
        let ev = make_evidence(EvidenceType::VulnScan, observed);
        let after = ev.expires_at + Duration::seconds(1);
        assert!(!is_fresh(&ev, after));
    }

    #[test]
    fn each_evidence_type_correct_period() {
        // Exhaustively verify all 9 evidence types return the expected duration
        let cases = [
            (EvidenceType::ChaosExperiment, 90),
            (EvidenceType::GameDay, 180),
            (EvidenceType::PenTest, 365),
            (EvidenceType::AuditFinding, 365),
            (EvidenceType::VulnScan, 30),
            (EvidenceType::DoraMetric, 30),
            (EvidenceType::UnitTest, 30),
            (EvidenceType::Scorecard, 14),
            (EvidenceType::IntegrationTest, 60),
        ];
        for (et, expected_days) in cases {
            assert_eq!(
                freshness_period(&et).num_days(),
                expected_days,
                "Freshness period mismatch for {et}"
            );
        }
    }

    #[test]
    fn compute_expires_at_each_type() {
        use chrono::TimeZone;
        let observed = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let cases = [
            (EvidenceType::ChaosExperiment, 90),
            (EvidenceType::GameDay, 180),
            (EvidenceType::PenTest, 365),
            (EvidenceType::AuditFinding, 365),
            (EvidenceType::VulnScan, 30),
            (EvidenceType::DoraMetric, 30),
            (EvidenceType::UnitTest, 30),
            (EvidenceType::Scorecard, 14),
            (EvidenceType::IntegrationTest, 60),
        ];
        for (et, expected_days) in cases {
            let expires = compute_expires_at(observed, &et);
            assert_eq!(
                (expires - observed).num_days(),
                expected_days,
                "compute_expires_at mismatch for {et}"
            );
        }
    }

    #[test]
    fn is_fresh_just_before_expiry() {
        let observed = Utc::now();
        let ev = make_evidence(EvidenceType::Scorecard, observed);
        let just_before = ev.expires_at - Duration::seconds(1);
        assert!(is_fresh(&ev, just_before));
    }

    #[test]
    fn is_fresh_one_millisecond_after_expiry() {
        let observed = Utc::now();
        let ev = make_evidence(EvidenceType::Scorecard, observed);
        let just_after = ev.expires_at + Duration::milliseconds(1);
        assert!(!is_fresh(&ev, just_after));
    }

    #[test]
    fn is_fresh_at_observation_time() {
        let observed = Utc::now();
        let ev = make_evidence(EvidenceType::GameDay, observed);
        assert!(is_fresh(&ev, observed));
    }
}
