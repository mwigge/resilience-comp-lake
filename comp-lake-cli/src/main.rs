use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;

/// Compliance data lake for resilience testing.
#[derive(Parser)]
#[command(name = "comp-lake", version, about)]
struct Cli {
    /// Path to `DuckDB` database file (default: in-memory)
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Load seed frameworks and mappings into the database
    Seed {
        /// Path to seed data directory
        #[arg(long, default_value = "data/seed")]
        data_dir: PathBuf,
    },
    /// Compute compliance scores for an entity
    Score {
        /// Entity ID to score
        #[arg(long)]
        entity: String,
        /// Framework ID (omit for all frameworks)
        #[arg(long)]
        framework: Option<String>,
    },
    /// Show coverage gaps for an entity
    Gaps {
        /// Entity ID
        #[arg(long)]
        entity: String,
    },
    /// Start the REST API server
    Serve {
        /// Port to listen on
        #[arg(long, default_value = "8080")]
        port: u16,
    },
    /// Load demo org hierarchy and sample evidence
    Demo,
    /// Show database statistics
    Stats,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let store = if let Some(ref path) = cli.db {
        comp_lake_storage::store::CompLakeStore::open(path)
            .context("failed to open database")?
    } else {
        comp_lake_storage::store::CompLakeStore::in_memory()
            .context("failed to create in-memory database")?
    };

    comp_lake_storage::views::create_views(store.conn())
        .context("failed to create views")?;

    match cli.command {
        Commands::Seed { data_dir } => cmd_seed(&store, &data_dir),
        Commands::Score { entity, framework } => cmd_score(&store, &entity, framework.as_deref()),
        Commands::Gaps { entity } => cmd_gaps(&store, &entity),
        Commands::Demo => cmd_demo(&store),
        Commands::Serve { port } => {
            cmd_serve(port);
            Ok(())
        }
        Commands::Stats => cmd_stats(&store),
    }
}

fn cmd_seed(
    store: &comp_lake_storage::store::CompLakeStore,
    data_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let fw_dir = data_dir.join("frameworks");
    let map_dir = data_dir.join("mappings");

    // Load framework seed files
    let mut total_frameworks = 0;
    let mut total_controls = 0;

    if fw_dir.exists() {
        for entry in std::fs::read_dir(&fw_dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "toml") {
                let result = comp_lake_harvesters::manual::load_seed_file(&path)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                store.upsert_framework(&result.framework)?;
                for ctrl in &result.controls {
                    store.upsert_control(ctrl)?;
                }
                total_frameworks += 1;
                total_controls += result.controls.len();

                println!(
                    "  loaded {} — {} controls",
                    result.framework.framework_id, result.controls.len()
                );
            }
        }
    }

    // Load mapping seed files
    let mappings = comp_lake_harvesters::mapping_loader::load_all_mappings(&map_dir)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut loaded_mappings = 0;
    for m in &mappings {
        if store.upsert_mapping(m).is_ok() {
            loaded_mappings += 1;
        }
    }

    println!(
        "\nSeeded {total_frameworks} frameworks, {total_controls} controls, {loaded_mappings}/{} mappings",
        mappings.len()
    );
    Ok(())
}

fn cmd_score(
    store: &comp_lake_storage::store::CompLakeStore,
    entity: &str,
    framework: Option<&str>,
) -> anyhow::Result<()> {
    let (query, params): (&str, Vec<String>) = if let Some(fw) = framework {
        (
            "SELECT framework_id, score, controls_total, controls_passing, controls_stale, badge \
             FROM v_scores WHERE entity_id = ? AND framework_id = ? ORDER BY framework_id",
            vec![entity.to_owned(), fw.to_owned()],
        )
    } else {
        (
            "SELECT framework_id, score, controls_total, controls_passing, controls_stale, badge \
             FROM v_scores WHERE entity_id = ? ORDER BY framework_id",
            vec![entity.to_owned()],
        )
    };

    let mut stmt = store.conn().prepare(query)?;
    let param_refs: Vec<&dyn duckdb::ToSql> = params.iter().map(|p| p as &dyn duckdb::ToSql).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;

    println!("Compliance scores for entity: {entity}\n");
    println!(
        "{:<20} {:>6} {:>8} {:>8} {:>6} {:>10}",
        "Framework", "Score", "Total", "Pass", "Stale", "Badge"
    );
    println!("{}", "-".repeat(62));

    let mut found = false;
    for row in rows {
        let (fw_id, pct, ctrl_total, ctrl_passing, ctrl_stale, badge) = row?;
        if ctrl_total > 0 {
            println!(
                "{fw_id:<20} {pct:>5.1}% {ctrl_total:>8} {ctrl_passing:>8} {ctrl_stale:>6} {badge:>10}"
            );
            found = true;
        }
    }

    if !found {
        println!("  (no scores found — load seed data with `comp-lake seed` first)");
    }
    Ok(())
}

fn cmd_gaps(
    store: &comp_lake_storage::store::CompLakeStore,
    entity: &str,
) -> anyhow::Result<()> {
    let mut stmt = store.conn().prepare(
        "SELECT priority_rank, framework_name, control_id, title, severity, gap_reason \
         FROM v_coverage_gaps WHERE entity_id = ? ORDER BY priority_rank LIMIT 20",
    )?;
    let rows = stmt.query_map([entity], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;

    println!("Top coverage gaps for entity: {entity}\n");
    println!(
        "{:>4} {:<12} {:<16} {:<30} {:>8} {:<14}",
        "#", "Framework", "Control", "Title", "Severity", "Gap Reason"
    );
    println!("{}", "-".repeat(90));

    for row in rows {
        let (rank, fw, ctrl, title, sev, reason) = row?;
        let title_short = if title.len() > 28 {
            format!("{}...", &title[..25])
        } else {
            title
        };
        println!(
            "{rank:>4} {fw:<12} {ctrl:<16} {title_short:<30} {sev:>8} {reason:<14}"
        );
    }
    Ok(())
}

fn cmd_demo(store: &comp_lake_storage::store::CompLakeStore) -> anyhow::Result<()> {
    use chrono::Utc;
    use comp_lake_core::models::control::ControlId;
    use comp_lake_core::models::evidence::{
        Evidence, EvidenceId, EvidenceResult, EvidenceType, SourceSystem,
    };
    use comp_lake_core::models::freshness::compute_expires_at;
    use comp_lake_core::models::org::{EntityId, EntityType, OrgEntity};

    // Create org hierarchy
    let entities = [
        ("acme-platform", EntityType::Platform, "ACME Platform", None),
        ("eng-unit", EntityType::Unit, "Engineering", Some("acme-platform")),
        ("team-alpha", EntityType::Team, "Team Alpha", Some("eng-unit")),
        ("team-beta", EntityType::Team, "Team Beta", Some("eng-unit")),
        ("proj-payments", EntityType::Project, "Payments Service", Some("team-alpha")),
        ("proj-auth", EntityType::Project, "Auth Service", Some("team-alpha")),
        ("proj-gateway", EntityType::Project, "API Gateway", Some("team-beta")),
    ];

    for (id, etype, name, parent) in &entities {
        store.upsert_org_entity(&OrgEntity::new(
            EntityId::new(*id),
            *etype,
            *name,
            parent.map(EntityId::new),
        ))?;
    }
    println!("Created {} org entities", entities.len());

    // Create sample evidence — some DORA controls pass, some fail, some untested
    let now = Utc::now();
    let passing_controls = [
        "DORA-ART-5", "DORA-ART-9", "DORA-ART-10", "DORA-ART-11",
        "DORA-ART-17", "DORA-ART-24", "DORA-ART-25",
    ];
    let failing_controls = ["DORA-ART-19", "DORA-ART-28"];
    let partial_controls = ["DORA-ART-26"];

    let mut ev_count = 0;
    for entity in ["proj-payments", "proj-auth", "proj-gateway"] {
        for ctrl_id in &passing_controls {
            let et = EvidenceType::ChaosExperiment;
            store.upsert_evidence(&Evidence {
                evidence_id: EvidenceId::new(),
                entity_id: EntityId::new(entity),
                control_id: ControlId::new(*ctrl_id).unwrap(),
                evidence_type: et,
                source_system: SourceSystem::new("tumult"),
                result: EvidenceResult::Pass,
                score: Some(1.0),
                metadata: serde_json::json!({"source": "demo"}),
                observed_at: now,
                expires_at: compute_expires_at(now, &et),
            })?;
            ev_count += 1;
        }
        for ctrl_id in &failing_controls {
            let et = EvidenceType::ChaosExperiment;
            store.upsert_evidence(&Evidence {
                evidence_id: EvidenceId::new(),
                entity_id: EntityId::new(entity),
                control_id: ControlId::new(*ctrl_id).unwrap(),
                evidence_type: et,
                source_system: SourceSystem::new("tumult"),
                result: EvidenceResult::Fail,
                score: Some(0.0),
                metadata: serde_json::json!({"source": "demo"}),
                observed_at: now,
                expires_at: compute_expires_at(now, &et),
            })?;
            ev_count += 1;
        }
        for ctrl_id in &partial_controls {
            let et = EvidenceType::GameDay;
            store.upsert_evidence(&Evidence {
                evidence_id: EvidenceId::new(),
                entity_id: EntityId::new(entity),
                control_id: ControlId::new(*ctrl_id).unwrap(),
                evidence_type: et,
                source_system: SourceSystem::new("tumult"),
                result: EvidenceResult::Partial,
                score: Some(0.5),
                metadata: serde_json::json!({"source": "demo"}),
                observed_at: now,
                expires_at: compute_expires_at(now, &et),
            })?;
            ev_count += 1;
        }
    }

    println!("Created {ev_count} evidence records across 3 projects");
    println!("\nTry:");
    println!("  comp-lake --db <db> score --entity proj-payments");
    println!("  comp-lake --db <db> score --entity proj-payments --framework DORA");
    println!("  comp-lake --db <db> gaps --entity proj-payments");
    Ok(())
}

fn cmd_serve(port: u16) {
    println!("REST API server would start on port {port}");
    println!("(Phase 3 — not yet implemented)");
}

fn cmd_stats(store: &comp_lake_storage::store::CompLakeStore) -> anyhow::Result<()> {
    println!("Database statistics:\n");
    for table in [
        "frameworks",
        "controls",
        "control_mappings",
        "org_hierarchy",
        "evidence",
        "harvest_log",
    ] {
        let count = store.count(table)?;
        println!("  {table:<20} {count:>6} rows");
    }
    Ok(())
}
