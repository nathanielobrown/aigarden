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
    /// `link-target`: relative markdown link/image targets exist on disk.
    #[serde(default)]
    pub link_target: RuleToggle,
    /// `link-case`: a link target's case matches the filesystem exactly.
    #[serde(default)]
    pub link_case: RuleToggle,
    /// `bare-path`: a backticked file-shaped path in prose exists.
    #[serde(default)]
    pub bare_path: RuleToggle,
    /// `import-target`: an `@path` import in a guidance file resolves.
    #[serde(default)]
    pub import_target: RuleToggle,
    /// `code-doc-ref`: a doc path cited in a non-markdown file exists.
    #[serde(default)]
    pub code_doc_ref: RuleToggle,
    /// `anchor-resolves`: a `#fragment` resolves to a real heading (rumdl MD051).
    #[serde(default)]
    pub anchor_resolves: RuleToggle,
    /// `markdown-style`: rumdl style rules surfaced under ailint config.
    #[serde(default)]
    pub markdown_style: MarkdownStyleConfig,
    /// `cog-fresh`: every generated cog block matches its generator's output.
    #[serde(default)]
    pub cog_fresh: RuleToggle,
}

/// A plain per-rule on/off switch — the shared shape for rules with no other
/// options. `enabled` defaults true so an absent table means "on".
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RuleToggle {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for RuleToggle {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// `markdown-style`: a small, curated slice of rumdl's style linting surfaced
/// under ailint keys, rather than exposing raw rumdl config. `reflow` maps to
/// rumdl's MD013 one-paragraph-per-line normalization; the rest are rumdl's
/// defaults, fixable via `ailint check --fix`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct MarkdownStyleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Normalize each paragraph to a single line (rumdl MD013 reflow), the
    /// convention the source repo uses. Off by default — it rewrites prose.
    #[serde(default)]
    pub reflow: bool,
}

impl Default for MarkdownStyleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            reflow: false,
        }
    }
}

/// Paths skipped by every rule — the single home for the fixtures/worktree
/// exemption that used to be re-coded per tool.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludeConfig {
    /// Glob patterns (globset syntax) matched against repo-relative paths.
    /// Added to the built-in defaults, not replacing them (see [`Self::effective_paths`]).
    #[serde(default)]
    pub paths: Vec<String>,
}

/// Built-in exclusions applied to every repo. `**/fixtures/**` because tracked
/// fixture dirs hold deliberately-odd/oversized files that trip hygiene rules and
/// that `.gitignore` (which covers build output, not tracked fixtures) won't skip.
fn default_excludes() -> Vec<String> {
    vec!["**/fixtures/**".to_string()]
}

impl ExcludeConfig {
    /// User paths unioned with the built-in defaults. Union, not override, so
    /// adding one exclude never silently drops fixtures protection.
    #[must_use]
    pub fn effective_paths(&self) -> Vec<String> {
        self.paths
            .iter()
            .cloned()
            .chain(default_excludes())
            .collect()
    }
}

/// Config for the `file-length` rule.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct FileLengthConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Append the generic built-in budgets after the user's. Set `false` to run
    /// **only** the user budgets — required to faithfully translate an
    /// exactly-scoped external gate (whose dirs the generic `**/*.py`/`**/*.md`
    /// defaults would over-cover). Defaults resolve first-match-wins after the
    /// user's, so they can otherwise only be shadowed, not removed.
    #[serde(default = "default_true")]
    pub use_defaults: bool,
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
            use_defaults: true,
            budget: Vec::new(),
        }
    }
}

impl FileLengthConfig {
    /// User budgets, then the built-in defaults (unless `use_defaults = false`);
    /// first matching glob wins.
    pub fn effective_budgets(&self) -> Vec<Budget> {
        let defaults = if self.use_defaults {
            default_budgets()
        } else {
            Vec::new()
        };
        self.budget.iter().cloned().chain(defaults).collect()
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
    fn fixtures_are_excluded_by_default() {
        // Tracked fixture dirs hold deliberately-oversized/odd files (a real
        // 754-line workflow fixture trips the 700-line default), and every hygiene
        // tool excludes them — so ailint does too, out of the box.
        let config = Config::default();
        assert!(
            config
                .exclude
                .effective_paths()
                .iter()
                .any(|p| p.contains("fixtures"))
        );
    }

    #[test]
    fn user_excludes_extend_the_defaults() {
        // User paths add to the built-in excludes, they don't replace them — a repo
        // that excludes one more dir keeps its fixtures protection.
        let config: Config = toml::from_str("[exclude]\npaths = [\"build/**\"]\n").unwrap();
        let eff = config.exclude.effective_paths();
        assert!(eff.iter().any(|p| p == "build/**"), "user path kept");
        assert!(
            eff.iter().any(|p| p.contains("fixtures")),
            "default still applied"
        );
    }

    #[test]
    fn unknown_key_is_rejected_loudly() {
        let err = toml::from_str::<Config>("[nonsense]\nfoo = 1\n").unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn use_defaults_false_drops_builtin_budgets() {
        // A repo that wants to translate an exactly-scoped external gate must be
        // able to opt out of the generic built-in budgets, not just shadow them.
        let config: Config = toml::from_str(
            "[file-length]\nuse-defaults = false\n\
             [[file-length.budget]]\nglob = \"backend/**/*.py\"\nmetric = \"lines\"\nmax = 700\n",
        )
        .unwrap();
        let budgets = config.file_length.effective_budgets();
        assert_eq!(budgets.len(), 1, "only the user budget, no defaults");
        assert_eq!(budgets[0].glob, "backend/**/*.py");
    }

    #[test]
    fn use_defaults_true_is_the_default() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.file_length.use_defaults);
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
