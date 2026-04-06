use duckdb::Connection;

use comp_lake_core::models::control::Control;
use comp_lake_core::models::evidence::Evidence;
use comp_lake_core::models::framework::Framework;
use comp_lake_core::models::mapping::ControlMapping;
use comp_lake_core::models::org::OrgEntity;

use crate::schema;

/// Storage layer wrapping a `DuckDB` connection.
pub struct CompLakeStore {
    conn: Connection,
}

impl CompLakeStore {
    /// Open an in-memory store with schema initialised.
    ///
    /// # Errors
    ///
    /// Returns a `DuckDB` error if connection or schema creation fails.
    pub fn in_memory() -> duckdb::Result<Self> {
        let conn = Connection::open_in_memory()?;
        schema::migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Open a file-backed store with schema initialised.
    ///
    /// # Errors
    ///
    /// Returns a `DuckDB` error if connection or schema creation fails.
    pub fn open(path: &std::path::Path) -> duckdb::Result<Self> {
        let conn = Connection::open(path)?;
        schema::migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Get a reference to the underlying connection.
    #[must_use]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Insert or replace a framework.
    ///
    /// # Errors
    ///
    /// Returns a `DuckDB` error on insert failure.
    pub fn upsert_framework(&self, fw: &Framework) -> duckdb::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO frameworks \
             (framework_id, name, version, region, authority, is_pivot, \
              effective_date, sunset_date, celex_id, eli_uri, harvest_source, last_harvested) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                fw.framework_id.as_str(),
                fw.name,
                fw.version,
                format!("{:?}", fw.region),
                fw.authority,
                fw.is_pivot,
                fw.effective_date.map(|d| d.format("%Y-%m-%d").to_string()),
                fw.sunset_date.map(|d| d.format("%Y-%m-%d").to_string()),
                fw.celex_id,
                fw.eli_uri,
                format!("{:?}", fw.harvest_source),
                fw.last_harvested.map(|d| d.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Insert or replace a control.
    ///
    /// # Errors
    ///
    /// Returns a `DuckDB` error on insert failure.
    pub fn upsert_control(&self, ctrl: &Control) -> duckdb::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO controls \
             (control_id, framework_id, article_ref, chapter_ref, title, description, \
              family, severity, testing_relevant, parent_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                ctrl.control_id.as_str(),
                ctrl.framework_id.as_str(),
                ctrl.article_ref,
                ctrl.chapter_ref,
                ctrl.title,
                ctrl.description,
                ctrl.family.as_ref().map(|f| f.as_str().to_owned()),
                format!("{:?}", ctrl.severity),
                ctrl.testing_relevant,
                ctrl.parent_id.as_ref().map(|p| p.as_str().to_owned()),
            ],
        )?;
        Ok(())
    }

    /// Insert or replace a control mapping.
    ///
    /// # Errors
    ///
    /// Returns a `DuckDB` error on insert failure.
    pub fn upsert_mapping(&self, m: &ControlMapping) -> duckdb::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO control_mappings \
             (source_control, target_control, relationship, confidence, direction, provenance) \
             VALUES (?, ?, ?, ?, ?, ?)",
            duckdb::params![
                m.source_control.as_str(),
                m.target_control.as_str(),
                format!("{:?}", m.relationship),
                format!("{:?}", m.confidence),
                format!("{:?}", m.direction),
                format!("{:?}", m.provenance),
            ],
        )?;
        Ok(())
    }

    /// Insert or replace an org entity.
    ///
    /// # Errors
    ///
    /// Returns a `DuckDB` error on insert failure.
    pub fn upsert_org_entity(&self, entity: &OrgEntity) -> duckdb::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO org_hierarchy \
             (entity_id, entity_type, name, parent_id) \
             VALUES (?, ?, ?, ?)",
            duckdb::params![
                entity.entity_id.as_str(),
                format!("{:?}", entity.entity_type),
                entity.name,
                entity.parent_id.as_ref().map(|p| p.as_str().to_owned()),
            ],
        )?;
        Ok(())
    }

    /// Insert or replace an evidence record.
    ///
    /// # Errors
    ///
    /// Returns a `DuckDB` error on insert failure.
    pub fn upsert_evidence(&self, ev: &Evidence) -> duckdb::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO evidence \
             (evidence_id, entity_id, control_id, evidence_type, source_system, \
              result, score, metadata, observed_at, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                ev.evidence_id.to_string(),
                ev.entity_id.as_str(),
                ev.control_id.as_str(),
                format!("{:?}", ev.evidence_type),
                ev.source_system.as_str(),
                format!("{:?}", ev.result),
                ev.score.unwrap_or(ev.result.as_score()),
                ev.metadata.to_string(),
                ev.observed_at.to_rfc3339(),
                ev.expires_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Count rows in a table.
    ///
    /// # Errors
    ///
    /// Returns a `DuckDB` error on query failure.
    pub fn count(&self, table: &str) -> duckdb::Result<usize> {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        self.conn.query_row(&sql, [], |row| row.get::<_, usize>(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use comp_lake_core::models::control::{ControlFamily, ControlId, Severity};
    use comp_lake_core::models::evidence::{
        EvidenceId, EvidenceResult, EvidenceType, SourceSystem,
    };
    use comp_lake_core::models::framework::{FrameworkId, HarvestSource, Region};
    use comp_lake_core::models::freshness::compute_expires_at;
    use comp_lake_core::models::mapping::{
        Confidence, MappingDirection, MappingProvenance, MappingRelationship,
    };
    use comp_lake_core::models::org::{EntityId, EntityType};

    fn test_store() -> CompLakeStore {
        CompLakeStore::in_memory().unwrap()
    }

    fn test_framework() -> Framework {
        Framework::builder(FrameworkId::new("DORA").unwrap(), "DORA")
            .version("2022/2554")
            .region(Region::Eu)
            .authority("EU/EP")
            .harvest_source(HarvestSource::CellarSparql)
            .build()
    }

    fn test_control() -> Control {
        Control::builder(
            ControlId::new("DORA-25").unwrap(),
            FrameworkId::new("DORA").unwrap(),
            "ICT incident management",
        )
        .severity(Severity::High)
        .family(ControlFamily::new("Resilience Testing"))
        .testing_relevant(true)
        .build()
    }

    #[test]
    fn framework_roundtrip() {
        let store = test_store();
        store.upsert_framework(&test_framework()).unwrap();
        assert_eq!(store.count("frameworks").unwrap(), 1);
        store.upsert_framework(&test_framework()).unwrap();
        assert_eq!(store.count("frameworks").unwrap(), 1);
    }

    #[test]
    fn control_roundtrip() {
        let store = test_store();
        store.upsert_framework(&test_framework()).unwrap();
        store.upsert_control(&test_control()).unwrap();
        assert_eq!(store.count("controls").unwrap(), 1);
    }

    #[test]
    fn mapping_roundtrip() {
        let store = test_store();
        store.upsert_framework(&test_framework()).unwrap();
        let fw2 = Framework::builder(FrameworkId::new("NIST").unwrap(), "NIST")
            .version("r5")
            .authority("NIST")
            .build();
        store.upsert_framework(&fw2).unwrap();
        store.upsert_control(&test_control()).unwrap();
        let ctrl2 = Control::builder(
            ControlId::new("NIST-IR-4").unwrap(),
            FrameworkId::new("NIST").unwrap(),
            "Incident Handling",
        )
        .severity(Severity::High)
        .testing_relevant(true)
        .build();
        store.upsert_control(&ctrl2).unwrap();
        let mapping = ControlMapping::new(
            ControlId::new("NIST-IR-4").unwrap(),
            ControlId::new("DORA-25").unwrap(),
            MappingRelationship::Equivalent,
            Confidence::High,
            MappingDirection::Bidirectional,
            MappingProvenance::NistOlir,
        );
        store.upsert_mapping(&mapping).unwrap();
        assert_eq!(store.count("control_mappings").unwrap(), 1);
    }

    #[test]
    fn org_entity_roundtrip() {
        let store = test_store();
        let entity = OrgEntity::new(
            EntityId::new("team-alpha"),
            EntityType::Team,
            "Team Alpha",
            None,
        );
        store.upsert_org_entity(&entity).unwrap();
        assert_eq!(store.count("org_hierarchy").unwrap(), 1);
    }

    #[test]
    fn evidence_roundtrip() {
        let store = test_store();
        store.upsert_framework(&test_framework()).unwrap();
        store.upsert_control(&test_control()).unwrap();
        let entity = OrgEntity::new(
            EntityId::new("team-alpha"),
            EntityType::Team,
            "Team Alpha",
            None,
        );
        store.upsert_org_entity(&entity).unwrap();

        let now = Utc::now();
        let et = EvidenceType::ChaosExperiment;
        let ev = Evidence {
            evidence_id: EvidenceId::new(),
            entity_id: EntityId::new("team-alpha"),
            control_id: ControlId::new("DORA-25").unwrap(),
            evidence_type: et,
            source_system: SourceSystem::new("tumult"),
            result: EvidenceResult::Pass,
            score: Some(1.0),
            metadata: serde_json::json!({"experiment": "db-failover"}),
            observed_at: now,
            expires_at: compute_expires_at(now, &et),
        };
        store.upsert_evidence(&ev).unwrap();
        assert_eq!(store.count("evidence").unwrap(), 1);
        store.upsert_evidence(&ev).unwrap();
        assert_eq!(store.count("evidence").unwrap(), 1);
    }

    #[test]
    fn file_backed_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.duckdb");
        {
            let store = CompLakeStore::open(&path).unwrap();
            store.upsert_framework(&test_framework()).unwrap();
            assert_eq!(store.count("frameworks").unwrap(), 1);
        }
        {
            let store = CompLakeStore::open(&path).unwrap();
            assert_eq!(store.count("frameworks").unwrap(), 1);
        }
    }
}
