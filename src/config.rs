//! `aigarden.toml`: discovery, typed deserialization, and the strong defaults that
//! let a missing or empty file just work.
//!
//! Ruff-shaped, so the spellings transfer: top-level `exclude`/`extend-exclude`,
//! a flat `ignore` list plus a `[per-file-ignores]` map for turning rules off, and
//! `[file-length] budgets`/`extend-budgets` maps. Two invariants the rest of the
//! tool leans on:
//! - **Excludes are defined once** ([`Config::effective_excludes`]) and shared by
//!   every rule and by `mv` — never re-implemented per rule.
//! - **Unknown keys are rejected** (`deny_unknown_fields` throughout) so a typo'd
//!   config fails loudly at startup instead of silently doing nothing.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use globset::{Glob, GlobSet};
use indexmap::IndexMap;
use serde::Deserialize;

use crate::rules::descriptive_anchor::anchored_pattern;
use crate::walk::build_glob_set;

/// The whole `aigarden.toml` schema. Every field defaults, so `Config::default()`
/// is a fully-working configuration.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    /// Path globs that **replace** the built-in default excludes ([`default_excludes`]).
    /// Absent ⇒ the built-ins apply. Ruff semantics: pair with `extend-exclude` to
    /// add on top rather than replace.
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
    /// Path globs appended to the effective base excludes (the built-ins, or
    /// `exclude` when set). May coexist with `exclude`.
    #[serde(default)]
    pub extend_exclude: Vec<String>,
    /// Rule names disabled repo-wide. An unknown name is a load-time config error.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// `"glob" = ["rule", …]`: for a matching file, the **union** of every matching
    /// entry's rules is disabled (order-independent, ruff-style). Globs and rule
    /// names are validated eagerly at load.
    #[serde(default)]
    pub per_file_ignores: IndexMap<String, Vec<String>>,
    /// The `file-length` rule's budgets.
    #[serde(default)]
    pub file_length: FileLengthConfig,
    /// `markdown-style`: rumdl style rules surfaced under aigarden config.
    #[serde(default)]
    pub markdown_style: MarkdownStyleConfig,
    /// `descriptive-anchor`: a stable-ID link must carry descriptive text.
    #[serde(default)]
    pub descriptive_anchor: DescriptiveAnchorConfig,
    /// `status-header`: the terminal-status "frozen docs" contract and exemption.
    /// The config type lives with its rule ([`crate::rules::status_header`]).
    #[serde(default)]
    pub status_header: crate::rules::status_header::StatusHeaderConfig,
}

/// `descriptive-anchor`: config-driven, generic, and **inert until configured**.
/// With no `patterns` it emits nothing, so it is safe to leave on by default.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct DescriptiveAnchorConfig {
    /// Regexes for stable-ID shapes (e.g. `ADR-\d+`, `T\d+`, `P\d+`). A markdown
    /// link whose visible text is *only* one of these — a bare ID with no
    /// descriptive words — is flagged, unless it sits inside a prose parenthetical
    /// (a citation) or already carries an em dash (already descriptive).
    #[serde(default)]
    pub patterns: Vec<String>,
}

/// `markdown-style`: a small, curated slice of rumdl's style linting surfaced
/// under aigarden keys, rather than exposing raw rumdl config. `reflow` maps to
/// rumdl's MD013 one-paragraph-per-line normalization; the rest are rumdl's
/// defaults, fixable via `aigarden check --fix`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct MarkdownStyleConfig {
    /// Normalize each paragraph to a single line (rumdl MD013 reflow), the
    /// convention the source repo uses. Off by default — it rewrites prose.
    #[serde(default)]
    pub reflow: bool,
}

/// Built-in exclusions applied to every repo. `**/fixtures/**` because tracked
/// fixture dirs hold deliberately-odd/oversized files that trip hygiene rules and
/// that `.gitignore` (which covers build output, not tracked fixtures) won't skip.
fn default_excludes() -> Vec<String> {
    vec!["**/fixtures/**".to_string()]
}

impl Config {
    /// The effective exclude globs: the base (`exclude` if set, else the built-ins),
    /// plus `extend-exclude` appended. The single home for the walked-file universe.
    #[must_use]
    pub fn effective_excludes(&self) -> Vec<String> {
        let base = self.exclude.clone().unwrap_or_else(default_excludes);
        base.into_iter()
            .chain(self.extend_exclude.iter().cloned())
            .collect()
    }
}

/// Config for the `file-length` rule. Budgets are a `"glob" = { lines | tokens }`
/// map; declaration order is first-match order (see [`Self::effective_budgets`]).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct FileLengthConfig {
    /// Budgets that **replace** the built-in defaults ([`default_budgets`]). Absent
    /// ⇒ the built-ins apply.
    #[serde(default)]
    pub budgets: Option<IndexMap<String, BudgetValue>>,
    /// Budgets checked **before** the effective base (so a user entry shadows a
    /// default glob), then extend coverage to new globs without re-listing the base.
    #[serde(default)]
    pub extend_budgets: IndexMap<String, BudgetValue>,
}

impl FileLengthConfig {
    /// The ordered budget list: `extend-budgets` (declaration order) then the base
    /// (`budgets` if set, else the built-ins). First matching glob wins, so
    /// `extend-budgets` shadows the base. Values are validated at config load, so
    /// the metric resolution here cannot fail.
    pub fn effective_budgets(&self) -> Vec<Budget> {
        let base = match &self.budgets {
            Some(map) => budgets_from_map(map),
            None => default_budgets(),
        };
        budgets_from_map(&self.extend_budgets)
            .into_iter()
            .chain(base)
            .collect()
    }
}

/// One `file-length` budget value: an inline table with **exactly one** of `lines`
/// or `tokens` (the ceiling for that metric). Zero or both is a load-time config
/// error ([`Self::resolve`]); an unknown key is rejected by `deny_unknown_fields`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct BudgetValue {
    #[serde(default)]
    pub lines: Option<usize>,
    #[serde(default)]
    pub tokens: Option<usize>,
}

impl BudgetValue {
    /// The `(metric, max)` this value names, or a config error if it sets neither or
    /// both keys — a budget must measure exactly one size.
    fn resolve(&self) -> Result<(Metric, usize)> {
        match (self.lines, self.tokens) {
            (Some(lines), None) => Ok((Metric::Lines, lines)),
            (None, Some(tokens)) => Ok((Metric::Tokens, tokens)),
            (None, None) => bail!(
                "a `file-length` budget must set exactly one of `lines`/`tokens`, but neither is set"
            ),
            (Some(_), Some(_)) => {
                bail!(
                    "a `file-length` budget must set exactly one of `lines`/`tokens`, but both are set"
                )
            }
        }
    }
}

/// Flatten a validated `"glob" = { lines | tokens }` map into ordered [`Budget`]s.
/// The map preserves document order (indexmap), so first-match order is the order
/// the entries appear in `aigarden.toml`.
fn budgets_from_map(map: &IndexMap<String, BudgetValue>) -> Vec<Budget> {
    map.iter()
        .map(|(glob, value)| {
            let (metric, max) = value
                .resolve()
                .expect("budget values validated at config load");
            Budget {
                glob: glob.clone(),
                metric,
                max,
            }
        })
        .collect()
}

/// One resolved `(glob, metric, max)` budget group — the input the `file-length`
/// rule compiles and matches against. Built from the config map, never deserialized
/// directly (the wire shape is [`BudgetValue`]).
#[derive(Debug, Clone)]
pub struct Budget {
    /// globset pattern matched against repo-relative paths.
    pub glob: String,
    /// Which size to measure: source `lines` or context `tokens`.
    pub metric: Metric,
    /// Inclusive ceiling: a value strictly greater than `max` is a finding.
    pub max: usize,
}

/// The size a `file-length` budget measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Resolves rule enablement and the file-length budget list. Ruff-shaped and flat:
/// a rule is off for a file when it is in `ignore` or in the union of every matching
/// `[per-file-ignores]` entry. Per-file-ignores globs are compiled once at
/// construction; a malformed glob is a loud tool error (fail fast), never a no-op.
pub(crate) struct Resolver<'a> {
    config: &'a Config,
    /// Repo-wide disabled rule names.
    ignore: HashSet<&'a str>,
    /// One compiled matcher per `[per-file-ignores]` entry, paired with its rules.
    per_file: Vec<(GlobSet, &'a [String])>,
}

impl<'a> Resolver<'a> {
    pub(crate) fn new(config: &'a Config) -> Result<Self> {
        let ignore = config.ignore.iter().map(String::as_str).collect();
        let per_file = config
            .per_file_ignores
            .iter()
            .map(|(glob, rules)| {
                let set = build_glob_set(std::slice::from_ref(glob))
                    .with_context(|| format!("compiling `per-file-ignores` glob `{glob}`"))?;
                Ok((set, rules.as_slice()))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            config,
            ignore,
            per_file,
        })
    }

    /// True when `rule` runs on `path`: not in `ignore`, and not in the union of
    /// every matching `[per-file-ignores]` entry's rules.
    pub(crate) fn is_enabled(&self, rule: &str, path: &str) -> bool {
        if self.ignore.contains(rule) {
            return false;
        }
        !self
            .per_file
            .iter()
            .any(|(set, rules)| set.is_match(path) && rules.iter().any(|r| r == rule))
    }

    /// The effective ordered `file-length` budget list (global — no per-path budget
    /// resolution; per-path scoping is enablement only, via [`Self::is_enabled`]).
    pub(crate) fn file_length_budgets(&self) -> Vec<Budget> {
        self.config.file_length.effective_budgets()
    }

    pub(crate) fn markdown_style(&self) -> &'a MarkdownStyleConfig {
        &self.config.markdown_style
    }

    pub(crate) fn descriptive_anchor(&self) -> &'a DescriptiveAnchorConfig {
        &self.config.descriptive_anchor
    }
}

/// The loaded config plus the directory it was found in (the display root).
pub struct Loaded {
    pub config: Config,
    /// Directory containing `aigarden.toml`, or the cwd when none was found.
    pub root: PathBuf,
}

impl Config {
    /// Load config: an explicit `--config` file, else the nearest `aigarden.toml`
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
            let candidate = dir.join("aigarden.toml");
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

impl Config {
    /// Fail fast on any config value a rule would otherwise compile lazily (and
    /// panic on): every budget value and glob, every `descriptive-anchor` pattern,
    /// and every rule name in `ignore`/`per-file-ignores` (validated against the
    /// live registry, so there is no hand-kept second list). A bad value is a clean
    /// config error (exit 2) naming the offending key.
    fn validate(&self) -> Result<()> {
        // Budget values (exactly one metric) and their globs.
        let mut budget_maps: Vec<&IndexMap<String, BudgetValue>> =
            vec![&self.file_length.extend_budgets];
        budget_maps.extend(self.file_length.budgets.as_ref());
        for map in budget_maps {
            for (glob, value) in map {
                value
                    .resolve()
                    .with_context(|| format!("invalid `file-length` budget for glob `{glob}`"))?;
                Glob::new(glob)
                    .with_context(|| format!("invalid `file-length` budget glob `{glob}`"))?;
            }
        }
        for pattern in &self.descriptive_anchor.patterns {
            anchored_pattern(pattern)
                .with_context(|| format!("invalid `descriptive-anchor` pattern `{pattern}`"))?;
        }
        for glob in &self.status_header.files {
            Glob::new(glob)
                .with_context(|| format!("invalid `status-header` files glob `{glob}`"))?;
        }
        // Rule names in `ignore` / `per-file-ignores` must be real rules — a typo
        // would silently disable nothing, so reject it loudly against the registry.
        let known: Vec<&str> = crate::rules::rule_names();
        let check_rule = |rule: &str, whence: &str| -> Result<()> {
            if !known.contains(&rule) {
                bail!(
                    "`{whence}` names unknown rule `{rule}`. Known rules: {}",
                    known.join(", ")
                );
            }
            Ok(())
        };
        for rule in &self.ignore {
            check_rule(rule, "ignore")?;
        }
        for (glob, rules) in &self.per_file_ignores {
            // A malformed per-file-ignores glob is caught eagerly here too.
            Glob::new(glob).with_context(|| format!("invalid `per-file-ignores` glob `{glob}`"))?;
            for rule in rules {
                check_rule(rule, &format!("per-file-ignores[\"{glob}\"]"))?;
            }
        }
        // Only a frozen-aware rule can honor the exemption; a name outside that set
        // would silently do nothing, so reject it loudly (fail fast).
        for rule in &self.status_header.suppresses {
            if !crate::rules::status_header::FROZEN_AWARE_RULES.contains(&rule.as_str()) {
                bail!(
                    "`status-header.suppresses` names `{rule}`, which is not a frozen-aware rule \
                     (one of: {})",
                    crate::rules::status_header::FROZEN_AWARE_RULES.join(", ")
                );
            }
        }
        Ok(())
    }
}

fn parse_file(path: &Path) -> Result<Config> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading config file {}", path.display()))?;
    let config: Config =
        toml::from_str(&text).with_context(|| format!("parsing config file {}", path.display()))?;
    config
        .validate()
        .with_context(|| format!("in config file {}", path.display()))?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_yields_working_defaults() {
        // A missing/empty file just works: no excludes overridden, the three
        // built-in budgets in force, fixtures excluded.
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.file_length.effective_budgets().len(), 3);
        assert!(
            config
                .effective_excludes()
                .iter()
                .any(|p| p.contains("fixtures")),
            "fixtures excluded by default"
        );
    }

    #[test]
    fn exclude_replaces_defaults_extend_preserves_them() {
        // `exclude` replaces the built-ins (so fixtures protection is dropped unless
        // relisted); `extend-exclude` adds on top of the effective base. Both coexist.
        let replaced: Config = toml::from_str("exclude = [\"build/**\"]\n").unwrap();
        let eff = replaced.effective_excludes();
        assert_eq!(eff, vec!["build/**"], "exclude replaces, not unions");

        let extended: Config = toml::from_str("extend-exclude = [\"build/**\"]\n").unwrap();
        let eff = extended.effective_excludes();
        assert!(
            eff.iter().any(|p| p == "build/**"),
            "extend keeps the user path"
        );
        assert!(
            eff.iter().any(|p| p.contains("fixtures")),
            "extend preserves the built-in base"
        );

        let both: Config =
            toml::from_str("exclude = [\"a/**\"]\nextend-exclude = [\"b/**\"]\n").unwrap();
        assert_eq!(
            both.effective_excludes(),
            vec!["a/**", "b/**"],
            "both coexist: exclude is the base, extend appends"
        );
    }

    #[test]
    fn unknown_top_level_key_is_rejected_loudly() {
        let err = toml::from_str::<Config>("[nonsense]\nfoo = 1\n").unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn unknown_rule_in_ignore_is_a_config_error() {
        // A typo'd rule name in `ignore` would silently disable nothing — rejected
        // at load, listing the known rules.
        let config: Config = toml::from_str("ignore = [\"no-such-rule\"]\n").unwrap();
        let err = config.validate().unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no-such-rule"), "names the bad rule: {msg}");
        assert!(msg.contains("code-doc-ref"), "lists known rules: {msg}");
    }

    #[test]
    fn unknown_rule_in_per_file_ignores_is_a_config_error() {
        let config: Config =
            toml::from_str("[per-file-ignores]\n\"src/**\" = [\"nonsense\"]\n").unwrap();
        let err = config.validate().unwrap_err();
        assert!(format!("{err:#}").contains("nonsense"), "{err}");
    }

    #[test]
    fn a_budget_with_zero_or_both_metrics_is_a_config_error() {
        // Exactly one of lines/tokens: neither and both are each rejected at load.
        let neither: Config = toml::from_str("[file-length.budgets]\n\"**/*.rs\" = {}\n").unwrap();
        assert!(neither.validate().is_err(), "zero metrics rejected");

        let both: Config =
            toml::from_str("[file-length.budgets]\n\"**/*.rs\" = { lines = 1, tokens = 1 }\n")
                .unwrap();
        assert!(both.validate().is_err(), "both metrics rejected");
    }

    #[test]
    fn a_budget_with_an_unknown_key_is_rejected() {
        // `deny_unknown_fields` on the inline value guards a typo'd metric key.
        let err = toml::from_str::<Config>(
            "[file-length.budgets]\n\"**/*.rs\" = { lines = 1, max = 2 }\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn per_file_ignores_disables_a_rule_only_for_matching_files() {
        // The per-path mechanism: an entry disables its rules for matching files;
        // files outside the glob keep the rule on. Multiple matching globs union.
        let config: Config = toml::from_str(
            "[per-file-ignores]\n\
             \"tests/**\" = [\"code-doc-ref\"]\n\
             \"tests/special.rs\" = [\"bare-path\"]\n",
        )
        .unwrap();
        let resolver = Resolver::new(&config).unwrap();
        // A file matched by both entries has the *union* disabled.
        assert!(!resolver.is_enabled("code-doc-ref", "tests/special.rs"));
        assert!(!resolver.is_enabled("bare-path", "tests/special.rs"));
        // A file matched by only the first keeps bare-path on.
        assert!(!resolver.is_enabled("code-doc-ref", "tests/cli.rs"));
        assert!(resolver.is_enabled("bare-path", "tests/cli.rs"));
        // An unmatched file keeps everything on.
        assert!(resolver.is_enabled("code-doc-ref", "src/engine.rs"));
    }

    #[test]
    fn ignore_disables_a_rule_repo_wide() {
        let config: Config = toml::from_str("ignore = [\"bare-path\"]\n").unwrap();
        let resolver = Resolver::new(&config).unwrap();
        assert!(!resolver.is_enabled("bare-path", "anywhere.md"));
        assert!(resolver.is_enabled("link-target", "anywhere.md"));
    }

    #[test]
    fn budgets_replaces_the_builtins() {
        // `budgets` (no `extend-`) drops the three built-in defaults entirely.
        let config: Config =
            toml::from_str("[file-length.budgets]\n\"backend/**/*.py\" = { lines = 700 }\n")
                .unwrap();
        let budgets = config.file_length.effective_budgets();
        assert_eq!(budgets.len(), 1, "only the user budget, no defaults");
        assert_eq!(budgets[0].glob, "backend/**/*.py");
        assert_eq!(budgets[0].metric, Metric::Lines);
    }

    #[test]
    fn extend_budgets_shadows_a_builtin_and_preserves_declaration_order() {
        // `extend-budgets` are checked before the base (first-match-wins), and their
        // own declaration order is preserved — the load-bearing ordering guarantee.
        // Declaration order (`docs/**` first) deliberately differs from sorted order
        // (`**/*.md` first), so a regression to a sorting map fails this test.
        let config: Config = toml::from_str(
            "[file-length.extend-budgets]\n\
             \"docs/**/*.md\" = { tokens = 200 }\n\
             \"**/*.md\" = { tokens = 100 }\n",
        )
        .unwrap();
        let budgets = config.file_length.effective_budgets();
        // Two user entries lead, in declaration order, then the three built-ins.
        assert_eq!(budgets.len(), 5);
        assert_eq!(budgets[0].glob, "docs/**/*.md", "first user entry first");
        assert_eq!(budgets[0].max, 200);
        assert_eq!(budgets[1].glob, "**/*.md", "second user entry second");
        // The user `**/*.md` shadows the built-in `**/*.md` (8000) because every
        // extend entry is checked before the base — first match wins.
        assert_eq!(budgets[1].max, 100);
    }

    #[test]
    fn descriptive_anchor_is_inert_until_configured() {
        // With no patterns the rule can never fire — safe to ship on.
        let config = Config::default();
        assert!(config.descriptive_anchor.patterns.is_empty());
    }

    #[test]
    fn status_header_suppresses_validation_is_unchanged() {
        // Only a frozen-aware rule can be suppressed; naming file-length is a loud
        // config error at load, exactly as before the schema change.
        let config: Config = toml::from_str(
            "[status-header]\nfiles = [\"issues/**/*.md\"]\nterminal = [\"done\"]\n\
             suppresses = [\"file-length\"]\n",
        )
        .unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            format!("{err:#}").contains("not a frozen-aware rule"),
            "{err}"
        );
    }
}
