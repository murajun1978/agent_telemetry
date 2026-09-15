use std::{path::PathBuf, sync::Arc};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

use agent_telemetry::{
    adapters::{greptime::GreptimeStore, jsonl},
    core::store::{EventQuery, TelemetryStore},
};

#[cfg(feature = "duckdb-backend")]
use agent_telemetry::adapters::duckdb::DuckDbStore;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Backend {
    Greptime,
    #[cfg(feature = "duckdb-backend")]
    Duckdb,
}

#[derive(Parser)]
#[command(name = "atel", version, about = "Agent Telemetry CLI")]
struct Cli {
    #[arg(long, value_enum, env = "ATEL_BACKEND", default_value = "greptime")]
    backend: Backend,

    #[arg(
        long,
        env = "ATEL_GREPTIME_ENDPOINT",
        default_value = "http://127.0.0.1:4000"
    )]
    greptime_endpoint: String,

    #[arg(long, env = "ATEL_GREPTIME_DATABASE", default_value = "public")]
    greptime_database: String,

    #[cfg(feature = "duckdb-backend")]
    #[arg(
        long,
        env = "ATEL_DUCKDB_PATH",
        default_value = "agent_telemetry.duckdb"
    )]
    duckdb_path: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init,
    Import {
        path: PathBuf,
    },
    Recent {
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = build_store(&cli)?;

    match cli.command {
        Command::Init => {
            store.init().await?;
            println!("initialized {}", store.name());
        }
        Command::Import { path } => {
            store.init().await?;
            let events = jsonl::read_events(path).await?;
            store.append(&events).await?;
            println!("imported {} events into {}", events.len(), store.name());
        }
        Command::Recent {
            agent,
            session,
            limit,
        } => {
            let events = store
                .recent(&EventQuery {
                    agent,
                    session_id: session,
                    limit,
                })
                .await?;
            for event in events {
                println!("{}", serde_json::to_string(&event)?);
            }
        }
    }

    Ok(())
}

fn build_store(cli: &Cli) -> Result<Arc<dyn TelemetryStore>> {
    match cli.backend {
        Backend::Greptime => Ok(Arc::new(GreptimeStore::new(
            &cli.greptime_endpoint,
            &cli.greptime_database,
        ))),
        #[cfg(feature = "duckdb-backend")]
        Backend::Duckdb => Ok(Arc::new(DuckDbStore::open(&cli.duckdb_path)?)),
        #[allow(unreachable_patterns)]
        _ => bail!("backend is not enabled in this build"),
    }
}
