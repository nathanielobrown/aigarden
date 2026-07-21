//! `ailint.toml`: discovery, typed deserialization, and the strong defaults that
//! let a missing or empty file just work.
//!
//! Two invariants the rest of the tool leans on:
//! - **Excludes are defined once** ([`ExcludeConfig`]) and shared by every rule
//!   and (later) by `mv` — never re-implemented per rule.
//! - **Unknown keys are rejected** (`deny_unknown_fields` throughout) so a typo'd
//!   config fails loudly at startup instead of silently doing nothing.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The whole `ailint.toml` schema. Every field defaults, so `Config::default()`
/// is a fully-working configuration.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    /// Tool-level path exclusions layered on top of `.gitignore`.
    #[serde(default)]
    pub exclude: ExcludeConfig,
    /// The `file-length` rule's budgets and toggle.
    #[serde(default)]
    pub file_length: FileLengthConfig,
}

/// Paths skipped by every rule — the single home for the fixtures/worktree
/// exemption that used to be re-coded per tool.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludeConfig {
    /// Glob patterns (globset syntax) matched against repo-relative paths.
    #[serde(default)]
    pub paths: Vec<String>,
}

/// Config for the `file-length` rule.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct FileLengthConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// User budgets, checked **before** the built-in defaults (first match wins),
    /// so a user entry both overrides a default glob and extends coverage to new
    /// globs without re-listing the defaults.
    #[serde(default)]
    pub budget: Vec<Budget>,
}

impl Default for FileLengthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            budget: Vec::new(),
        }
    }
}

impl FileLengthConfig {
    /// User budgets followed by the built-in defaults; first matching glob wins.
    pub fn effective_budgets(&self) -> Vec<Budget> {
        self.budget
            .iter()
            .cloned()
            .chain(default_budgets())
            .collect()
    }
}

/// One `(glob, metric, max)` budget group.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// globset pattern matched against repo-relative paths.
    pub glob: String,
    /// Which size to measure: source `lines` or context `tokens`.
    pub metric: Metric,
    /// Inclusive ceiling: a value strictly greater than `max` is a finding.
    pub max: usize,
}

/// The size a `file-length` budget measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Metric {
    /// True line count (trailing-newline-insensitive) — for human-read code.
    Lines,
    /// `ceil(chars / 4)` — the context cost of always-loaded / prose files.
    Tokens,
}

impl Metric {
    pub fn as_str(self) -> &'static str {
        match self {
            Metric::Lines => "lines",
            Metric::Tokens => "tokens",
        }
    }
}

/// Built-in budgets, generic (no repo-specific globs). Order matters: guidance
/// and markdown match on `*.md` before the code group, so they win.
fn default_budgets() -> Vec<Budget> {
    vec![
        // Always-loaded AI-guidance files: budget context tokens, not lines.
        Budget {
            glob: "**/{CLAUDE,AGENTS,GEMINI,SKILL}.md".to_string(),
            metric: Metric::Tokens,
            max: 4000,
        },
        // General markdown docs: also context-cost budgeted, more generous.
        Budget {
            glob: "**/*.md".to_string(),
            metric: Metric::Tokens,
            max: 8000,
        },
        // Source files: budget human-readable line count.
        Budget {
            glob: "**/*.{rs,py,ts,tsx,js,jsx,vue,go,java,kt,c,cc,cpp,h,hpp,rb,swift,sh}"
                .to_string(),
            metric: Metric::Lines,
            max: 700,
        },
    ]
}

fn default_true() -> bool {
    true
}

/// The loaded config plus the directory it was found in (the display root).
pub struct Loaded {
    pub config: Config,
    /// Directory containing `ailint.toml`, or the cwd when none was found.
    pub root: PathBuf,
}

impl Config {
    /// Load config: an explicit `--config` file, else the nearest `ailint.toml`
    /// walking up from `cwd`, else strong defaults rooted at `cwd`.
    pub fn discover(explicit: Option<&Path>, cwd: &Path) -> Result<Loaded> {
        if let Some(path) = explicit {
            let config = parse_file(path)?;
            let root = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map_or_else(|| cwd.to_path_buf(), Path::to_path_buf);
            return Ok(Loaded { config, root });
        }
        let mut dir = cwd;
        loop {
            let candidate = dir.join("ailint.toml");
            if candidate.is_file() {
                return Ok(Loaded {
                    config: parse_file(&candidate)?,
                    root: dir.to_path_buf(),
                });
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
        Ok(Loaded {
            config: Config::default(),
            root: cwd.to_path_buf(),
        })
    }
}

fn parse_file(path: &Path) -> Result<Config> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading config file {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing config file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_yields_working_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.file_length.enabled);
        assert!(config.exclude.paths.is_empty());
        // Defaults still cover code and markdown.
        assert_eq!(config.file_length.effective_budgets().len(), 3);
    }

    #[test]
    fn unknown_key_is_rejected_loudly() {
        let err = toml::from_str::<Config>("[nonsense]\nfoo = 1\n").unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn user_budget_is_checked_before_defaults() {
        let config: Config = toml::from_str(
            "[[file-length.budget]]\nglob = \"**/*.rs\"\nmetric = \"lines\"\nmax = 1000\n",
        )
        .unwrap();
        let budgets = config.file_length.effective_budgets();
        // User entry leads, then the three defaults.
        assert_eq!(budgets.len(), 4);
        assert_eq!(budgets[0].max, 1000);
    }
}
