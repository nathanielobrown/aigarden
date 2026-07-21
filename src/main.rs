//! `ailint` — lint and maintain repositories for AI-agent + human collaboration.
//!
//! This is the CLI shell only: every subcommand is wired for shape but exits
//! "not implemented" until its rule logic lands. No rule engine here yet.

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

/// Lint and maintain repositories for AI-agent + human collaboration.
#[derive(Parser)]
#[command(name = "ailint", version, about)]
struct Cli {
    /// How to render diagnostics: annotated text for humans, or machine output for agents/CI.
    // Read once the output layer lands; #[expect] reminds us to drop this the moment it is.
    #[expect(dead_code, reason = "consumed when the output layer is implemented")]
    #[arg(long, value_enum, global = true, default_value_t = OutputFormat::Human)]
    output_format: OutputFormat,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run every lint layer over the repository (link integrity, size budgets, cog freshness).
    Check {
        /// Apply fixes in place where a rule supports them.
        #[arg(long)]
        fix: bool,
    },
    /// Check or rewrite generated cog blocks.
    Cog {
        /// Rewrite cog blocks in place.
        #[arg(long, conflicts_with = "check")]
        write: bool,
        /// Fail if any cog block is stale, without rewriting.
        #[arg(long)]
        check: bool,
    },
    /// Move a file and rewrite every reference to it across the repository.
    Mv {
        /// Path to move.
        src: String,
        /// Destination path.
        dst: String,
    },
    /// List the available rules and their status.
    Rules,
}

/// Output rendering format, selected with `--output-format`.
#[derive(Copy, Clone, Debug, ValueEnum)]
enum OutputFormat {
    /// Annotated source snippets for human reading (default).
    Human,
    /// Structured JSON for agents and tooling.
    Json,
    /// GitHub Actions workflow-command annotations.
    Github,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Check { .. } => bail!("`ailint check` is not implemented yet"),
        Command::Cog { .. } => bail!("`ailint cog` is not implemented yet"),
        Command::Mv { .. } => bail!("`ailint mv` is not implemented yet"),
        Command::Rules => bail!("`ailint rules` is not implemented yet"),
    }
}
