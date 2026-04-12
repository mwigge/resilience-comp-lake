use std::collections::HashMap;

use super::badges::BadgeTier;
use super::ComplianceScore;
use crate::models::framework::FrameworkId;
use crate::models::org::{EntityId, EntityType, OrgEntity};

/// Roll up entity-level scores to parent levels in the org hierarchy.
///
/// For each parent entity, its score is the arithmetic mean of its children's scores.
/// Returns scores for all entities that have children with scores.
#[must_use]
pub fn rollup_scores(
    entity_scores: &[(EntityId, ComplianceScore)],
    hierarchy: &[OrgEntity],
) -> Vec<(EntityId, ComplianceScore)> {
    // Build parent -> children map
    let mut children_map: HashMap<&EntityId, Vec<&EntityId>> = HashMap::new();
    for entity in hierarchy {
        if let Some(parent_id) = &entity.parent_id {
            children_map
                .entry(parent_id)
                .or_default()
                .push(&entity.entity_id);
        }
    }

    // Build entity_id -> score map
    let score_map: HashMap<&EntityId, &ComplianceScore> = entity_scores
        .iter()
        .map(|(id, score)| (id, score))
        .collect();

    // Build entity_id -> entity map for type lookups
    let entity_map: HashMap<&EntityId, &OrgEntity> =
        hierarchy.iter().map(|e| (&e.entity_id, e)).collect();

    // Process parents in order: Team, Unit, Platform (bottom-up)
    let mut all_scores: HashMap<EntityId, ComplianceScore> = entity_scores
        .iter()
        .map(|(id, s)| (id.clone(), s.clone()))
        .collect();

    for level in [EntityType::Team, EntityType::Unit, EntityType::Platform] {
        let parents_at_level: Vec<&EntityId> = hierarchy
            .iter()
            .filter(|e| e.entity_type == level)
            .map(|e| &e.entity_id)
            .collect();

        for parent_id in parents_at_level {
            if let Some(child_ids) = children_map.get(parent_id) {
                let child_scores: Vec<&ComplianceScore> = child_ids
                    .iter()
                    .filter_map(|cid| {
                        all_scores
                            .get(*cid)
                            .or_else(|| score_map.get(*cid).copied())
                    })
                    .collect();

                if !child_scores.is_empty() {
                    // Get framework_id from first child (all should be same framework)
                    let fw_id = child_scores[0].framework_id.clone();
                    let rolled_up = mean_score(&fw_id, &child_scores);
                    all_scores.insert(parent_id.clone(), rolled_up);
                }
            }
        }
    }

    // Return only the rolled-up parent scores (not the leaf scores that were input)
    let input_ids: std::collections::HashSet<&EntityId> =
        entity_scores.iter().map(|(id, _)| id).collect();

    let _ = entity_map; // used for future extensions

    all_scores
        .into_iter()
        .filter(|(id, _)| !input_ids.contains(id))
        .collect()
}

/// Aggregate multiple framework scores into a single summary score (arithmetic mean).
#[must_use]
pub fn aggregate_score(
    framework_id: &FrameworkId,
    framework_scores: &[ComplianceScore],
) -> ComplianceScore {
    if framework_scores.is_empty() {
        return ComplianceScore::new(framework_id.clone(), 0, 0, 0, 0);
    }

    let refs: Vec<&ComplianceScore> = framework_scores.iter().collect();
    mean_score(framework_id, &refs)
}

fn mean_score(framework_id: &FrameworkId, scores: &[&ComplianceScore]) -> ComplianceScore {
    if scores.is_empty() {
        return ComplianceScore::new(framework_id.clone(), 0, 0, 0, 0);
    }

    let total: usize = scores.iter().map(|s| s.controls_total).sum();
    let covered: usize = scores.iter().map(|s| s.controls_covered).sum();
    let passing: usize = scores.iter().map(|s| s.controls_passing).sum();
    let stale: usize = scores.iter().map(|s| s.controls_stale).sum();

    #[allow(clippy::cast_precision_loss)]
    let avg_score = scores.iter().map(|s| s.score).sum::<f64>() / scores.len() as f64;
    let badge = BadgeTier::from_score(avg_score);

    ComplianceScore {
        framework_id: framework_id.clone(),
        score: avg_score,
        controls_total: total,
        controls_covered: covered,
        controls_passing: passing,
        controls_stale: stale,
        badge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::org::EntityType;

    fn fw() -> FrameworkId {
        FrameworkId::new("DORA").unwrap()
    }

    fn make_score(entity: &str, pct: f64) -> (EntityId, ComplianceScore) {
        let total: usize = 10;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let passing = ((pct / 100.0) * total as f64).round() as usize;
        (
            EntityId::new(entity),
            ComplianceScore::new(fw(), total, passing, passing, 0),
        )
    }

    fn make_entity(id: &str, entity_type: EntityType, parent: Option<&str>) -> OrgEntity {
        OrgEntity::new(
            EntityId::new(id),
            entity_type,
            id,
            parent.map(EntityId::new),
        )
    }

    #[test]
    fn team_score_is_mean_of_projects() {
        let hierarchy = vec![
            make_entity("team-a", EntityType::Team, Some("unit-eng")),
            make_entity("proj-1", EntityType::Project, Some("team-a")),
            make_entity("proj-2", EntityType::Project, Some("team-a")),
        ];
        let scores = vec![make_score("proj-1", 80.0), make_score("proj-2", 60.0)];

        let rolled = rollup_scores(&scores, &hierarchy);
        let team_score = rolled.iter().find(|(id, _)| id.as_str() == "team-a");
        assert!(team_score.is_some());
        let (_, s) = team_score.unwrap();
        assert!((s.score - 70.0).abs() < f64::EPSILON);
    }

    #[test]
    fn unit_score_is_mean_of_teams() {
        let hierarchy = vec![
            make_entity("unit-eng", EntityType::Unit, Some("platform")),
            make_entity("team-a", EntityType::Team, Some("unit-eng")),
            make_entity("team-b", EntityType::Team, Some("unit-eng")),
            make_entity("proj-1", EntityType::Project, Some("team-a")),
            make_entity("proj-2", EntityType::Project, Some("team-b")),
        ];
        let scores = vec![make_score("proj-1", 90.0), make_score("proj-2", 70.0)];

        let rolled = rollup_scores(&scores, &hierarchy);

        let team_a = rolled.iter().find(|(id, _)| id.as_str() == "team-a");
        assert!(team_a.is_some());
        assert!((team_a.unwrap().1.score - 90.0).abs() < f64::EPSILON);

        let unit = rolled.iter().find(|(id, _)| id.as_str() == "unit-eng");
        assert!(unit.is_some());
        // Mean of team-a(90) and team-b(70) = 80
        assert!((unit.unwrap().1.score - 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_score_averages_frameworks() {
        let scores = vec![
            ComplianceScore::new(FrameworkId::new("DORA").unwrap(), 10, 8, 8, 0),
            ComplianceScore::new(FrameworkId::new("NIST").unwrap(), 20, 10, 10, 2),
        ];
        let agg = aggregate_score(&FrameworkId::new("ALL").unwrap(), &scores);
        // DORA score = 80.0, NIST score = 50.0, mean = 65.0
        assert!((agg.score - 65.0).abs() < f64::EPSILON);
        assert_eq!(agg.controls_total, 30);
        assert_eq!(agg.controls_passing, 18);
    }

    #[test]
    fn empty_children_scores_zero() {
        let hierarchy = vec![make_entity("team-a", EntityType::Team, Some("unit-eng"))];
        let scores: Vec<(EntityId, ComplianceScore)> = vec![];
        let rolled = rollup_scores(&scores, &hierarchy);
        // No children have scores, so no rollup produced
        assert!(rolled.is_empty());
    }

    #[test]
    fn empty_aggregate_scores_zero() {
        let agg = aggregate_score(&FrameworkId::new("ALL").unwrap(), &[]);
        assert!((agg.score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn platform_score_is_mean_of_units() {
        let hierarchy = vec![
            make_entity("platform", EntityType::Platform, None),
            make_entity("unit-a", EntityType::Unit, Some("platform")),
            make_entity("unit-b", EntityType::Unit, Some("platform")),
            make_entity("team-a1", EntityType::Team, Some("unit-a")),
            make_entity("team-b1", EntityType::Team, Some("unit-b")),
            make_entity("proj-a1", EntityType::Project, Some("team-a1")),
            make_entity("proj-b1", EntityType::Project, Some("team-b1")),
        ];
        let scores = vec![make_score("proj-a1", 100.0), make_score("proj-b1", 60.0)];

        let rolled = rollup_scores(&scores, &hierarchy);

        let platform_score = rolled.iter().find(|(id, _)| id.as_str() == "platform");
        assert!(platform_score.is_some());
        // team-a1 = 100, team-b1 = 60
        // unit-a = 100, unit-b = 60
        // platform = mean(100, 60) = 80
        let (_, s) = platform_score.unwrap();
        assert!((s.score - 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn single_child_parent_equals_child() {
        let hierarchy = vec![
            make_entity("team-a", EntityType::Team, Some("unit-x")),
            make_entity("proj-1", EntityType::Project, Some("team-a")),
        ];
        let scores = vec![make_score("proj-1", 73.0)];

        let rolled = rollup_scores(&scores, &hierarchy);
        let team_score = rolled.iter().find(|(id, _)| id.as_str() == "team-a");
        assert!(team_score.is_some());
        // Single child: parent score == child score with no rounding artefacts
        let (_, s) = team_score.unwrap();
        // make_score rounds: 73% of 10 = 7.3 -> round to 7, 7/10 = 70%
        // So we check exact match to child score
        let child_score = scores[0].1.score;
        assert!((s.score - child_score).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_children_produces_no_rollup_not_nan() {
        // Parent exists but no child scores provided
        let hierarchy = vec![
            make_entity("team-a", EntityType::Team, Some("unit-x")),
            make_entity("proj-1", EntityType::Project, Some("team-a")),
        ];
        let scores: Vec<(EntityId, ComplianceScore)> = vec![];
        let rolled = rollup_scores(&scores, &hierarchy);
        // No scores to aggregate means no rollup entry (NOT NaN)
        assert!(rolled.is_empty());
    }

    #[test]
    fn aggregate_single_score_equals_itself() {
        let single = ComplianceScore::new(FrameworkId::new("DORA").unwrap(), 10, 8, 8, 1);
        let agg = aggregate_score(
            &FrameworkId::new("AGG").unwrap(),
            std::slice::from_ref(&single),
        );
        assert!((agg.score - single.score).abs() < f64::EPSILON);
        assert_eq!(agg.controls_total, single.controls_total);
        assert_eq!(agg.controls_passing, single.controls_passing);
        assert_eq!(agg.controls_stale, single.controls_stale);
    }

    #[test]
    fn rollup_full_hierarchy_bottom_to_top() {
        // Platform -> Unit -> Team -> Projects
        let hierarchy = vec![
            make_entity("platform", EntityType::Platform, None),
            make_entity("unit-eng", EntityType::Unit, Some("platform")),
            make_entity("team-a", EntityType::Team, Some("unit-eng")),
            make_entity("team-b", EntityType::Team, Some("unit-eng")),
            make_entity("proj-1", EntityType::Project, Some("team-a")),
            make_entity("proj-2", EntityType::Project, Some("team-a")),
            make_entity("proj-3", EntityType::Project, Some("team-b")),
        ];
        // proj-1=80, proj-2=60 => team-a = 70
        // proj-3=90 => team-b = 90
        // unit-eng = mean(70, 90) = 80
        // platform = mean(80) = 80
        let scores = vec![
            make_score("proj-1", 80.0),
            make_score("proj-2", 60.0),
            make_score("proj-3", 90.0),
        ];

        let rolled = rollup_scores(&scores, &hierarchy);

        let team_a = rolled
            .iter()
            .find(|(id, _)| id.as_str() == "team-a")
            .map(|(_, s)| s.score);
        let team_b = rolled
            .iter()
            .find(|(id, _)| id.as_str() == "team-b")
            .map(|(_, s)| s.score);
        let unit = rolled
            .iter()
            .find(|(id, _)| id.as_str() == "unit-eng")
            .map(|(_, s)| s.score);
        let platform = rolled
            .iter()
            .find(|(id, _)| id.as_str() == "platform")
            .map(|(_, s)| s.score);

        assert!(team_a.is_some());
        assert!(team_b.is_some());
        assert!(unit.is_some());
        assert!(platform.is_some());

        assert!((team_a.unwrap() - 70.0).abs() < f64::EPSILON);
        assert!((team_b.unwrap() - 90.0).abs() < f64::EPSILON);
        assert!((unit.unwrap() - 80.0).abs() < f64::EPSILON);
        assert!((platform.unwrap() - 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rollup_aggregates_partial_correctly() {
        // Two projects where covered > passing (Partial evidence present).
        // proj-1: total=10, covered=8, passing=5  (3 partials, score=50.0)
        // proj-2: total=10, covered=6, passing=6  (0 partials, score=60.0)
        // team rollup should sum fields: total=20, covered=14, passing=11
        let hierarchy = vec![
            make_entity("team-a", EntityType::Team, Some("unit-x")),
            make_entity("proj-1", EntityType::Project, Some("team-a")),
            make_entity("proj-2", EntityType::Project, Some("team-a")),
        ];
        let scores = vec![
            (
                EntityId::new("proj-1"),
                ComplianceScore::new(fw(), 10, 8, 5, 0),
            ),
            (
                EntityId::new("proj-2"),
                ComplianceScore::new(fw(), 10, 6, 6, 0),
            ),
        ];

        let rolled = rollup_scores(&scores, &hierarchy);
        let team = rolled
            .iter()
            .find(|(id, _)| id.as_str() == "team-a")
            .map(|(_, s)| s)
            .expect("team-a should have a rollup score");

        assert_eq!(team.controls_total, 20, "total should sum both projects");
        assert_eq!(
            team.controls_covered, 14,
            "covered should sum both projects"
        );
        assert_eq!(
            team.controls_passing, 11,
            "passing should sum both projects"
        );
        // Verify covered != passing is preserved (partial evidence path)
        assert!(
            team.controls_covered > team.controls_passing,
            "rollup must preserve covered > passing when partial evidence exists"
        );
    }
}
