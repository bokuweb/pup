mod analysis;
mod build;
mod collect;
mod config;
mod http;
mod model;
mod sources;
mod wiki;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "pup",
    about = "Market moves around key economic events, and the OKF LLM wiki built on them"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Fetch prices and event calendars into the data directory.
    Collect {
        #[arg(long, default_value = "data")]
        data: PathBuf,
        #[arg(long, default_value = "2016-01-01")]
        since: chrono::NaiveDate,
    },
    /// Compute event windows and aggregates into published JSON.
    Build {
        #[arg(long, default_value = "data")]
        data: PathBuf,
        #[arg(long, default_value = "public/data")]
        out: PathBuf,
    },
    /// Choose events to write up and prepare source bundles and page skeletons.
    WikiPlan(wiki::plan::Args),
    /// Regenerate machine-written blocks, index and log; roll back unfinished tasks.
    WikiFinalize(wiki::finalize::Args),
    /// Validate OKF frontmatter, links and every number the LLM wrote against the facts.
    WikiCheck(wiki::check::Args),
    /// Convert the wiki branch to JSON for the SPA.
    WikiExport(wiki::export::Args),
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();
    match Cli::parse().cmd {
        Cmd::Collect { data, since } => collect::run(&data, since),
        Cmd::Build { data, out } => build::run(&data, &out),
        Cmd::WikiPlan(a) => wiki::plan::run(&a),
        Cmd::WikiFinalize(a) => wiki::finalize::run(&a),
        Cmd::WikiCheck(a) => wiki::check::run(&a),
        Cmd::WikiExport(a) => wiki::export::run(&a),
    }
}
