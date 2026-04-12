use chrono::{DateTime, Duration, Utc};

use crate::harvester::{HarvestCadence, HarvestConfig, HarvestError, HarvestResult, Harvester};
use comp_lake_core::models::framework::FrameworkId;

/// Determines if a harvester is due based on its cadence and last harvest time.
#[must_use]
pub fn is_due(
    cadence: HarvestCadence,
    last_harvested: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    let Some(last) = last_harvested else {
        return true; // never harvested
    };

    let interval = cadence_to_duration(cadence);
    now >= last + interval
}

/// Convert a harvest cadence to a `Duration`.
#[must_use]
pub fn cadence_to_duration(cadence: HarvestCadence) -> Duration {
    match cadence {
        HarvestCadence::Daily => Duration::days(1),
        HarvestCadence::Weekly => Duration::weeks(1),
        HarvestCadence::Monthly => Duration::days(30),
        HarvestCadence::OnRelease | HarvestCadence::OnVersion => Duration::days(365),
    }
}

/// Run a harvester, returning the result or error.
///
/// # Errors
///
/// Returns `HarvestError` if the harvest fails.
pub async fn run_harvester(
    harvester: &dyn Harvester,
    config: &HarvestConfig,
) -> Result<HarvestResult, HarvestError> {
    tracing::info!(harvester = harvester.name(), "starting harvest");
    let result = harvester.harvest(config).await;
    match &result {
        Ok(r) => tracing::info!(
            harvester = harvester.name(),
            controls = r.controls.len(),
            mappings = r.mappings.len(),
            "harvest completed"
        ),
        Err(e) => tracing::error!(harvester = harvester.name(), error = %e, "harvest failed"),
    }
    result
}

/// Find which harvesters from a list are due for execution.
#[must_use]
pub fn find_due_harvesters<'a>(
    harvesters: &'a [Box<dyn Harvester>],
    last_harvested: &dyn Fn(&FrameworkId) -> Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Vec<&'a dyn Harvester> {
    harvesters
        .iter()
        .filter(|h| {
            h.frameworks()
                .iter()
                .any(|fw_id| is_due(h.cadence(), last_harvested(fw_id), now))
        })
        .map(AsRef::as_ref)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_harvested_is_due() {
        assert!(is_due(HarvestCadence::Daily, None, Utc::now()));
    }

    #[test]
    fn daily_due_after_24h() {
        let now = Utc::now();
        let yesterday = now - Duration::days(1);
        assert!(is_due(HarvestCadence::Daily, Some(yesterday), now));
    }

    #[test]
    fn daily_not_due_within_24h() {
        let now = Utc::now();
        let recent = now - Duration::hours(12);
        assert!(!is_due(HarvestCadence::Daily, Some(recent), now));
    }

    #[test]
    fn weekly_due_after_7d() {
        let now = Utc::now();
        let week_ago = now - Duration::weeks(1);
        assert!(is_due(HarvestCadence::Weekly, Some(week_ago), now));
    }

    #[test]
    fn weekly_not_due_within_7d() {
        let now = Utc::now();
        let days_ago = now - Duration::days(3);
        assert!(!is_due(HarvestCadence::Weekly, Some(days_ago), now));
    }

    #[test]
    fn monthly_due_after_30d() {
        let now = Utc::now();
        let month_ago = now - Duration::days(31);
        assert!(is_due(HarvestCadence::Monthly, Some(month_ago), now));
    }

    #[test]
    fn on_release_due_after_365d() {
        let now = Utc::now();
        let year_ago = now - Duration::days(366);
        assert!(is_due(HarvestCadence::OnRelease, Some(year_ago), now));
    }

    #[test]
    fn cadence_durations() {
        assert_eq!(cadence_to_duration(HarvestCadence::Daily).num_days(), 1);
        assert_eq!(cadence_to_duration(HarvestCadence::Weekly).num_days(), 7);
        assert_eq!(cadence_to_duration(HarvestCadence::Monthly).num_days(), 30);
        assert_eq!(
            cadence_to_duration(HarvestCadence::OnRelease).num_days(),
            365
        );
    }
}
