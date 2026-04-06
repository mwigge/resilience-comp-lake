use clap::Parser;

/// Compliance data lake for resilience testing.
#[derive(Parser)]
#[command(name = "comp-lake", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Harvest framework data from upstream sources
    Harvest,
    /// Compute compliance scores
    Score,
    /// Export data as Parquet/CSV
    Export,
    /// Start the REST API + MCP server
    Serve,
    /// Validate framework data integrity
    Validate,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Harvest => anyhow::bail!("harvest: not yet implemented"),
        Commands::Score => anyhow::bail!("score: not yet implemented"),
        Commands::Export => anyhow::bail!("export: not yet implemented"),
        Commands::Serve => anyhow::bail!("serve: not yet implemented"),
        Commands::Validate => anyhow::bail!("validate: not yet implemented"),
    }
}
