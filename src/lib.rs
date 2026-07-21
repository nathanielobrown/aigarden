//! `ailint` — lint and maintain repositories for AI-agent + human collaboration.
//!
//! The library is the whole tool; `main.rs` is a one-line shell. [`run`] takes a
//! parsed [`Cli`] and returns a process [`ExitCode`]: 0 clean, 1 findings, 2
//! tool/config error. Every rule runs and every finding is reported in one pass.

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};

pub mod cli;
mod cog;
pub mod config;
pub mod diagnostic;
mod engine;
mod fix;
mod mv;
mod output;
pub mod references;
mod rules;
mod rumdl_adapter;
mod walk;

pub use cli::{Cli, Command, OutputFormat};

use config::{Config, Resolver};

/// Run a parsed CLI, rendering to stdout and returning the process exit code.
///
/// Tool/config errors (exit 2) are written to stderr here; findings (exit 1) and
/// the clean path (exit 0) are decided from the rendered diagnostics.
pub fn run(cli: &Cli) -> ExitCode {
    let mut stdout = io::stdout();
    match dispatch(cli, &mut stdout) {
        Ok(code) => code,
        Err(err) => {
            // Tool/config failures are loud on stderr and exit 2, distinct from findings.
            let mut stderr = io::stderr();
            let _ = writeln!(stderr, "ailint: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn dispatch(cli: &Cli, out: &mut impl Write) -> Result<ExitCode> {
    match &cli.command {
        Command::Check { paths, fix } => run_check(cli, paths, *fix, out),
        Command::Rules => {
            list_rules(out)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Cog { write, check } => run_cog(cli, *write, *check, out),
        Command::Mv { src, dst } => mv::run(cli, src, dst, out),
    }
}

/// Check or rewrite the repository's cog blocks. `--check` reports stale blocks as
/// diagnostics (exit 1) and surfaces a failing generator as a tool error (exit 2);
/// `--write` splices updates in place and reports what changed. Exactly one of the
/// two is required — there is no default mode.
fn run_cog(cli: &Cli, write: bool, check: bool, out: &mut impl Write) -> Result<ExitCode> {
    let cwd = env::current_dir()?;
    let loaded = Config::discover(cli.config.as_deref(), &cwd)?;
    let files = walk::walk(
        &[PathBuf::from(".")],
        &loaded.config.exclude.effective_paths(),
        &cwd,
    )?;
    // `check`/`write` are a required, mutually-exclusive pair (enforced by clap).
    if write {
        cog::write_repo(&files, &cwd, out)
    } else {
        debug_assert!(check);
        let _ = check;
        cog::check_repo(cli.output_format, &files, &cwd, out)
    }
}

fn run_check(cli: &Cli, paths: &[PathBuf], fix: bool, out: &mut impl Write) -> Result<ExitCode> {
    let cwd = env::current_dir()?;
    let loaded = Config::discover(cli.config.as_deref(), &cwd)?;
    let scan_paths: Vec<PathBuf> = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    let mut files = walk::walk(&scan_paths, &loaded.config.exclude.effective_paths(), &cwd)?;
    if files.is_empty() {
        // Fail fast: zero files under the requested paths is a misconfiguration, not a clean pass.
        bail!("no files found under {scan_paths:?}");
    }
    // `--fix` rewrites the auto-fixable markdown-style findings on disk first, then
    // the check below reports whatever remains — so a second `--fix` run is clean.
    // Per-file `markdown-style` config (enabled, reflow) is resolved inside `apply`.
    if fix {
        let resolver = Resolver::new(&loaded.config)?;
        fix::apply(&mut files, &resolver)?;
    }
    let diagnostics = engine::check(&files, &loaded.config, &cwd)?;
    output::render(cli.output_format, &diagnostics, files.len(), out)?;
    Ok(if diagnostics.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// List every registered rule, one `name — description` line, sorted by name.
fn list_rules(out: &mut impl Write) -> Result<()> {
    let mut rules: Vec<_> = rules::registry()
        .iter()
        .map(|r| (r.name(), r.description()))
        .collect();
    rules.sort_by_key(|(name, _)| *name);
    for (name, description) in rules {
        writeln!(out, "{name} \u{2014} {description}")?;
    }
    Ok(())
}
