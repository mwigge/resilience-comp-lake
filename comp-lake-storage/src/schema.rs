use duckdb::Connection;

/// Current schema version.
pub const SCHEMA_VERSION: &str = "1";

/// Create all tables if they do not exist.
///
/// # Errors
///
/// Returns a `DuckDB` error if any DDL statement fails.
pub fn create_schema(conn: &Connection) -> duckdb::Result<()> {
    conn.execute_batch(DDL)?;
    set_schema_version(conn, SCHEMA_VERSION)?;
    Ok(())
}

/// Read the current schema version, or `None` if `schema_meta` doesn't exist.
///
/// # Errors
///
/// Returns a `DuckDB` error on query failure.
pub fn schema_version(conn: &Connection) -> duckdb::Result<Option<String>> {
    let result = conn.query_row(
        "SELECT value FROM schema_meta WHERE key = 'schema_version'",
        [],
        |row| row.get::<_, String>(0),
    );
    match result {
        Ok(v) => Ok(Some(v)),
        Err(duckdb::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("does not exist") || msg.contains("Catalog Error") {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

/// Migrate schema to the current version. Currently only creates from scratch.
///
/// # Errors
///
/// Returns a `DuckDB` error if schema creation or migration fails.
pub fn migrate(conn: &Connection) -> duckdb::Result<()> {
    let version = schema_version(conn)?;
    match version.as_deref() {
        Some(SCHEMA_VERSION) => Ok(()),
        Some(_older) => {
            // Future: incremental migrations go here
            // For now, we only have version 1
            Ok(())
        }
        None => create_schema(conn),
    }
}

fn set_schema_version(conn: &Connection, version: &str) -> duckdb::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO schema_meta (key, value) VALUES ('schema_version', ?)",
        [version],
    )?;
    Ok(())
}

const DDL: &str = "
CREATE TABLE IF NOT EXISTS schema_meta (
    key   VARCHAR PRIMARY KEY,
    value VARCHAR NOT NULL
);

CREATE TABLE IF NOT EXISTS frameworks (
    framework_id   VARCHAR PRIMARY KEY,
    name           VARCHAR NOT NULL,
    version        VARCHAR NOT NULL,
    region         VARCHAR NOT NULL,
    authority      VARCHAR NOT NULL,
    is_pivot       BOOLEAN NOT NULL DEFAULT false,
    effective_date DATE,
    sunset_date    DATE,
    celex_id       VARCHAR,
    eli_uri        VARCHAR,
    harvest_source VARCHAR NOT NULL,
    last_harvested TIMESTAMP,
    created_at     TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at     TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS controls (
    control_id       VARCHAR PRIMARY KEY,
    framework_id     VARCHAR NOT NULL REFERENCES frameworks,
    article_ref      VARCHAR,
    chapter_ref      VARCHAR,
    title            VARCHAR NOT NULL,
    description      TEXT,
    family           VARCHAR,
    severity         VARCHAR NOT NULL,
    testing_relevant BOOLEAN NOT NULL DEFAULT false,
    parent_id        VARCHAR REFERENCES controls,
    created_at       TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at       TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS control_mappings (
    source_control VARCHAR NOT NULL REFERENCES controls,
    target_control VARCHAR NOT NULL REFERENCES controls,
    relationship   VARCHAR NOT NULL,
    confidence     VARCHAR NOT NULL,
    direction      VARCHAR NOT NULL DEFAULT 'bidirectional',
    provenance     VARCHAR NOT NULL,
    PRIMARY KEY (source_control, target_control)
);

CREATE TABLE IF NOT EXISTS org_hierarchy (
    entity_id   VARCHAR PRIMARY KEY,
    entity_type VARCHAR NOT NULL,
    name        VARCHAR NOT NULL,
    parent_id   VARCHAR REFERENCES org_hierarchy
);

CREATE TABLE IF NOT EXISTS evidence (
    evidence_id   VARCHAR PRIMARY KEY,
    entity_id     VARCHAR NOT NULL REFERENCES org_hierarchy,
    control_id    VARCHAR NOT NULL REFERENCES controls,
    evidence_type VARCHAR NOT NULL,
    source_system VARCHAR NOT NULL,
    result        VARCHAR NOT NULL,
    score         DOUBLE,
    metadata      TEXT,
    observed_at   TIMESTAMP NOT NULL,
    expires_at    TIMESTAMP NOT NULL
);

-- Indexes for analytical view performance
CREATE INDEX IF NOT EXISTS idx_evidence_entity_control ON evidence(entity_id, control_id);
CREATE INDEX IF NOT EXISTS idx_evidence_expires ON evidence(expires_at);
CREATE INDEX IF NOT EXISTS idx_controls_framework ON controls(framework_id, testing_relevant);
CREATE INDEX IF NOT EXISTS idx_org_parent ON org_hierarchy(parent_id);

CREATE TABLE IF NOT EXISTS harvest_log (
    harvest_id       VARCHAR PRIMARY KEY,
    framework_id     VARCHAR NOT NULL REFERENCES frameworks,
    started_at       TIMESTAMP NOT NULL,
    completed_at     TIMESTAMP,
    status           VARCHAR NOT NULL,
    controls_added   INTEGER DEFAULT 0,
    controls_updated INTEGER DEFAULT 0,
    mappings_added   INTEGER DEFAULT 0,
    error_message    TEXT
);
";

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn create_schema_on_empty_db() {
        let conn = in_memory();
        create_schema(&conn).unwrap();
        let version = schema_version(&conn).unwrap();
        assert_eq!(version.as_deref(), Some("1"));
    }

    #[test]
    fn create_schema_idempotent() {
        let conn = in_memory();
        create_schema(&conn).unwrap();
        create_schema(&conn).unwrap();
        let version = schema_version(&conn).unwrap();
        assert_eq!(version.as_deref(), Some("1"));
    }

    #[test]
    fn migrate_creates_schema_if_missing() {
        let conn = in_memory();
        migrate(&conn).unwrap();
        let version = schema_version(&conn).unwrap();
        assert_eq!(version.as_deref(), Some("1"));
    }

    #[test]
    fn migrate_noop_if_current() {
        let conn = in_memory();
        create_schema(&conn).unwrap();
        migrate(&conn).unwrap();
        let version = schema_version(&conn).unwrap();
        assert_eq!(version.as_deref(), Some("1"));
    }

    #[test]
    fn schema_version_none_on_empty_db() {
        let conn = in_memory();
        let version = schema_version(&conn).unwrap();
        assert_eq!(version, None);
    }

    #[test]
    fn foreign_keys_enforced() {
        let conn = in_memory();
        create_schema(&conn).unwrap();

        // Insert control referencing nonexistent framework should fail
        let result = conn.execute(
            "INSERT INTO controls (control_id, framework_id, title, severity, testing_relevant) \
             VALUES ('C1', 'NONEXISTENT', 'Test', 'HIGH', true)",
            [],
        );
        assert!(result.is_err());
    }
}
