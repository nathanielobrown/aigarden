//! Command-line surface: the `clap` types plus the `--output-format` enum.
//!
//! Parsing lives here so `main.rs` is a one-line shell and `run` is unit-testable.

use std::path::PathBuf;

use clap::{ArgGroup, Parser, Subcommand, ValueEnum};

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
        /// Apply fixes in place where a rule supports them (currently markdown-style).
        #[arg(long)]
        fix: bool,
    },
    /// Check or rewrite generated cog blocks (`<!-- ailint:cog … -->` regions).
    ///
    /// Pick exactly one mode — there is no default:
    ///   ailint cog --check    # fail if any block is stale (CI / the gate)
    ///   ailint cog --write    # regenerate every stale block in place
    #[command(group(ArgGroup::new("mode").required(true).args(["write", "check"])))]
    Cog {
        /// Regenerate every cog block in place, reporting which files changed.
        #[arg(long)]
        write: bool,
        /// Report stale cog blocks as diagnostics and exit non-zero, without writing.
        #[arg(long)]
        check: bool,
    },
    /// Move a file and rewrite every reference to it across the repository.
    ///
    /// Uses `git mv` for a tracked file (else a plain rename), rewrites markdown
    /// links, `@`-imports, backticked bare paths, and code doc-path citations, then
    /// re-runs the link rules to confirm the repo is still clean. Files only.
    ///   ailint mv docs/old.md docs/new.md      # rename in place
    ///   ailint mv notes.md archive/            # move into a directory
    Mv {
        /// File to move.
        src: String,
        /// Destination path, or a directory (trailing `/`) to move into.
        dst: String,
    },
    /// List the registered rules with their status and one-line descriptions.
    Rules,
    /// Print one rule's full contract: what it checks, its config keys, an example
    /// finding, and whether `--fix` repairs it.
    Explain {
        /// The rule name, as shown by `ailint rules` (e.g. `bare-path`).
        rule: String,
    },
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
