use anyhow::Context;
use clap::Parser;
use ledgernomics::LedgerService;
use rmcp::{transport::stdio, ServiceExt};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "ledgernomics", about = "Filesystem YAML ledger MCP")]
struct Cli {
    #[arg(long, env = "LEDGERNOMICS_ROOT", default_value = ".")]
    root: PathBuf,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("ledgernomics=warn,rmcp=warn")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cli = Cli::parse();
    let root = cli.root.canonicalize().unwrap_or(cli.root);
    let service = LedgerService::new(root);
    let server = service.serve(stdio()).await.context("serve stdio")?;
    server.waiting().await?;
    Ok(())
}
