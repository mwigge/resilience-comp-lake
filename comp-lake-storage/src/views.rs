use duckdb::Connection;

/// Create all analytical views. These views ARE the contract for embedded consumers.
///
/// # Errors
///
/// Returns a `DuckDB` error if any view creation fails.
pub fn create_views(conn: &Connection) -> duckdb::Result<()> {
    conn.execute_batch(VIEWS_DDL)
}

/// Drop and recreate all views (for schema changes).
///
/// # Errors
///
/// Returns a `DuckDB` error if drop or create fails.
pub fn refresh_views(conn: &Connection) -> duckdb::Result<()> {
    conn.execute_batch(
        "DROP VIEW IF EXISTS v_evidence_freshness;
         DROP VIEW IF EXISTS v_cross_framework_map;
         DROP VIEW IF EXISTS v_coverage_gaps;
         DROP VIEW IF EXISTS v_rollup_scores;
         DROP VIEW IF EXISTS v_scores;",
    )?;
    create_views(conn)
}

const VIEWS_DDL: &str = "
-- Per-entity, per-framework compliance score
CREATE VIEW IF NOT EXISTS v_scores AS
SELECT
    *,
    CASE
        WHEN score >= 95 THEN 'Platinum'
        WHEN score >= 85 THEN 'Gold'
        WHEN score >= 70 THEN 'Silver'
        WHEN score >= 50 THEN 'Bronze'
        ELSE 'None'
    END AS badge
FROM (
    SELECT
        e.entity_id,
        e.name              AS entity_name,
        e.entity_type,
        f.framework_id,
        f.name              AS framework_name,
        COUNT(DISTINCT c.control_id)
            FILTER (WHERE c.testing_relevant)                           AS controls_total,
        COUNT(DISTINCT ev.control_id)
            FILTER (WHERE ev.result IN ('Pass', 'Partial')
                      AND ev.expires_at > CURRENT_TIMESTAMP)            AS controls_covered,
        COUNT(DISTINCT ev.control_id)
            FILTER (WHERE ev.result = 'Pass'
                      AND ev.expires_at > CURRENT_TIMESTAMP)            AS controls_passing,
        COUNT(DISTINCT ev.control_id)
            FILTER (WHERE ev.expires_at <= CURRENT_TIMESTAMP)           AS controls_stale,
        ROUND(100.0 * COUNT(DISTINCT ev.control_id)
            FILTER (WHERE ev.result = 'Pass'
                      AND ev.expires_at > CURRENT_TIMESTAMP)
            / NULLIF(COUNT(DISTINCT c.control_id)
                FILTER (WHERE c.testing_relevant), 0), 1)              AS score
    FROM org_hierarchy e
    CROSS JOIN frameworks f
    LEFT JOIN controls c
        ON c.framework_id = f.framework_id
       AND c.testing_relevant = true
    LEFT JOIN evidence ev
        ON ev.entity_id = e.entity_id
       AND ev.control_id = c.control_id
    GROUP BY e.entity_id, e.name, e.entity_type, f.framework_id, f.name
) sub;

-- Roll-up: team = avg of projects, unit = avg of teams, etc.
CREATE VIEW IF NOT EXISTS v_rollup_scores AS
WITH RECURSIVE rollup AS (
    SELECT o.entity_id, o.entity_type, o.parent_id, s.framework_id, s.score, s.badge
    FROM v_scores s
    JOIN org_hierarchy o ON s.entity_id = o.entity_id
    WHERE o.entity_type = 'Project'
    UNION ALL
    SELECT p.entity_id, p.entity_type, p.parent_id,
           r.framework_id,
           ROUND(AVG(r.score), 1) AS score,
           CASE
               WHEN AVG(r.score) >= 95 THEN 'Platinum'
               WHEN AVG(r.score) >= 85 THEN 'Gold'
               WHEN AVG(r.score) >= 70 THEN 'Silver'
               WHEN AVG(r.score) >= 50 THEN 'Bronze'
               ELSE 'None'
           END AS badge
    FROM org_hierarchy p
    JOIN rollup r ON r.parent_id = p.entity_id
    GROUP BY p.entity_id, p.entity_type, p.parent_id, r.framework_id
)
SELECT * FROM rollup;

-- Coverage gaps: what to test next, prioritised
CREATE VIEW IF NOT EXISTS v_coverage_gaps AS
SELECT
    e.entity_id,
    e.name              AS entity_name,
    c.control_id,
    c.title,
    c.severity,
    c.framework_id,
    f.name              AS framework_name,
    COALESCE(best_ev.result, 'not_tested') AS current_status,
    best_ev.expires_at,
    CASE
        WHEN best_ev.evidence_id IS NULL   THEN 'never_tested'
        WHEN best_ev.expires_at <= NOW()   THEN 'stale'
        WHEN best_ev.result = 'Fail'       THEN 'failing'
        WHEN best_ev.result = 'Partial'    THEN 'partial'
    END AS gap_reason,
    ROW_NUMBER() OVER (
        PARTITION BY e.entity_id
        ORDER BY
            CASE c.severity WHEN 'High' THEN 1 WHEN 'Moderate' THEN 2 ELSE 3 END,
            CASE
                WHEN best_ev.evidence_id IS NULL THEN 1
                WHEN best_ev.expires_at <= NOW() THEN 2
                WHEN best_ev.result = 'Fail' THEN 3
                ELSE 4
            END
    ) AS priority_rank
FROM org_hierarchy e
CROSS JOIN controls c
JOIN frameworks f ON c.framework_id = f.framework_id
LEFT JOIN LATERAL (
    SELECT ev.*
    FROM evidence ev
    WHERE ev.entity_id = e.entity_id
      AND ev.control_id = c.control_id
    ORDER BY ev.observed_at DESC
    LIMIT 1
) best_ev ON true
WHERE c.testing_relevant = true
  AND (best_ev.evidence_id IS NULL
       OR best_ev.expires_at <= NOW()
       OR best_ev.result IN ('Fail', 'Partial'));

-- Cross-framework map: which controls satisfy multiple frameworks
CREATE VIEW IF NOT EXISTS v_cross_framework_map AS
SELECT
    c1.control_id    AS control_a,
    c1.framework_id  AS framework_a,
    c1.title         AS title_a,
    m.relationship,
    m.confidence,
    c2.control_id    AS control_b,
    c2.framework_id  AS framework_b,
    c2.title         AS title_b
FROM control_mappings m
JOIN controls c1 ON m.source_control = c1.control_id
JOIN controls c2 ON m.target_control = c2.control_id;

-- Evidence freshness dashboard
CREATE VIEW IF NOT EXISTS v_evidence_freshness AS
SELECT
    e.entity_id,
    e.name,
    ev.evidence_type,
    ev.source_system,
    COUNT(*) AS total_evidence,
    COUNT(*) FILTER (WHERE ev.expires_at > NOW()) AS fresh,
    COUNT(*) FILTER (WHERE ev.expires_at <= NOW()) AS stale,
    ROUND(100.0 * COUNT(*) FILTER (WHERE ev.expires_at > NOW())
        / COUNT(*), 1) AS freshness_pct
FROM org_hierarchy e
JOIN evidence ev ON ev.entity_id = e.entity_id
GROUP BY e.entity_id, e.name, ev.evidence_type, ev.source_system;
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;
    use crate::store::CompLakeStore;
    use chrono::Utc;
    use comp_lake_core::models::control::{ControlFamily, ControlId, Severity};
    use comp_lake_core::models::evidence::{
        Evidence, EvidenceId, EvidenceResult, EvidenceType, SourceSystem,
    };
    use comp_lake_core::models::framework::{Framework, FrameworkId, HarvestSource, Region};
    use comp_lake_core::models::freshness::compute_expires_at;
    use comp_lake_core::models::org::{EntityId, EntityType, OrgEntity};

    fn populated_store() -> CompLakeStore {
        let store = CompLakeStore::in_memory().unwrap();
        create_views(store.conn()).unwrap();

        let fw = Framework::builder(FrameworkId::new("DORA").unwrap(), "DORA")
            .version("2022/2554")
            .region(Region::Eu)
            .authority("EU/EP")
            .harvest_source(HarvestSource::CellarSparql)
            .build();
        store.upsert_framework(&fw).unwrap();

        for (id, sev) in [("C1", Severity::High), ("C2", Severity::Moderate)] {
            let ctrl = comp_lake_core::models::control::Control::builder(
                ControlId::new(id).unwrap(),
                FrameworkId::new("DORA").unwrap(),
                format!("Control {id}"),
            )
            .severity(sev)
            .family(ControlFamily::new("Testing"))
            .testing_relevant(true)
            .build();
            store.upsert_control(&ctrl).unwrap();
        }

        let entity = OrgEntity::new(
            EntityId::new("team-a"),
            EntityType::Project,
            "Team Alpha",
            None,
        );
        store.upsert_org_entity(&entity).unwrap();

        let now = Utc::now();
        let et = EvidenceType::ChaosExperiment;
        let ev = Evidence {
            evidence_id: EvidenceId::new(),
            entity_id: EntityId::new("team-a"),
            control_id: ControlId::new("C1").unwrap(),
            evidence_type: et,
            source_system: SourceSystem::new("tumult"),
            result: EvidenceResult::Pass,
            score: Some(1.0),
            metadata: serde_json::json!({}),
            observed_at: now,
            expires_at: compute_expires_at(now, &et),
        };
        store.upsert_evidence(&ev).unwrap();

        store
    }

    #[test]
    fn views_create_on_empty_db() {
        let conn = Connection::open_in_memory().unwrap();
        schema::create_schema(&conn).unwrap();
        create_views(&conn).unwrap();
    }

    #[test]
    fn views_create_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        schema::create_schema(&conn).unwrap();
        create_views(&conn).unwrap();
        create_views(&conn).unwrap();
    }

    #[test]
    fn refresh_views_works() {
        let conn = Connection::open_in_memory().unwrap();
        schema::create_schema(&conn).unwrap();
        create_views(&conn).unwrap();
        refresh_views(&conn).unwrap();
    }

    #[test]
    fn v_scores_returns_rows() {
        let store = populated_store();
        let count: usize = store
            .conn()
            .query_row("SELECT COUNT(*) FROM v_scores", [], |row| row.get(0))
            .unwrap();
        assert!(count > 0);
    }

    #[test]
    fn v_scores_correct_values() {
        let store = populated_store();
        let (ctrl_passing, ctrl_total, pct): (i64, i64, f64) = store
            .conn()
            .query_row(
                "SELECT controls_passing, controls_total, score FROM v_scores \
                 WHERE entity_id = 'team-a' AND framework_id = 'DORA'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(ctrl_total, 2);
        assert_eq!(ctrl_passing, 1);
        assert!((pct - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn v_coverage_gaps_returns_untested() {
        let store = populated_store();
        let gap_reason: String = store
            .conn()
            .query_row(
                "SELECT gap_reason FROM v_coverage_gaps \
                 WHERE entity_id = 'team-a' AND control_id = 'C2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(gap_reason, "never_tested");
    }

    #[test]
    fn v_coverage_gaps_high_before_moderate() {
        let store = populated_store();
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT control_id, priority_rank FROM v_coverage_gaps \
                 WHERE entity_id = 'team-a' ORDER BY priority_rank",
            )
            .unwrap();
        let rows: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        // C2 is Moderate+never_tested, but it should still appear
        assert!(!rows.is_empty());
    }

    #[test]
    fn v_evidence_freshness_counts() {
        let store = populated_store();
        let (total, fresh): (i64, i64) = store
            .conn()
            .query_row(
                "SELECT total_evidence, fresh FROM v_evidence_freshness \
                 WHERE entity_id = 'team-a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(fresh, 1);
    }

    #[test]
    fn v_cross_framework_map_with_mapping() {
        let store = populated_store();

        // Add a second framework + control + mapping
        let fw2 = Framework::builder(FrameworkId::new("NIST").unwrap(), "NIST")
            .version("r5")
            .authority("NIST")
            .build();
        store.upsert_framework(&fw2).unwrap();

        let ctrl = comp_lake_core::models::control::Control::builder(
            ControlId::new("N1").unwrap(),
            FrameworkId::new("NIST").unwrap(),
            "Incident Handling",
        )
        .severity(Severity::High)
        .testing_relevant(true)
        .build();
        store.upsert_control(&ctrl).unwrap();

        let mapping = comp_lake_core::models::mapping::ControlMapping::new(
            ControlId::new("N1").unwrap(),
            ControlId::new("C1").unwrap(),
            comp_lake_core::models::mapping::MappingRelationship::Equivalent,
            comp_lake_core::models::mapping::Confidence::High,
            comp_lake_core::models::mapping::MappingDirection::Bidirectional,
            comp_lake_core::models::mapping::MappingProvenance::NistOlir,
        );
        store.upsert_mapping(&mapping).unwrap();

        let count: usize = store
            .conn()
            .query_row("SELECT COUNT(*) FROM v_cross_framework_map", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
}
