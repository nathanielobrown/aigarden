//! Command-line surface: the `clap` types plus the `--output-format` enum.
//!
//! Parsing lives here so `main.rs` is a one-line shell and `run` is unit-testable.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Lint and maintain repositories for AI-agent + human collaboration.
#[derive(Parser, Debug)]
#[command(name = "ailint", version, about)]
pub struct Cli {
    /// How to render diagnostics: annotated text for humans, or machine output for agents/CI.
    #[arg(long, value_enum, global = true, default_value_t = OutputFormat::Human)]
    pub output_format: OutputFormat,

    /// Use this config file instead of discovering `ailint.toml` upward from the cwd.
    #[arg(long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run every lint rule over the repository and report all findings in one pass.
    Check {
        /// Paths to scan; defaults to the current directory.
        paths: Vec<PathBuf>,
        /// Apply fixes in place where a rule supports them (no rule does yet).
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
    /// List the registered rules and their one-line descriptions.
    Rules,
}

/// Output rendering format, selected with `--output-format`.
#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum OutputFormat {
    /// Annotated source snippets for human reading (default).
    Human,
    /// Structured JSON for agents and tooling.
    Json,
    /// GitHub Actions workflow-command annotations.
    Github,
}
