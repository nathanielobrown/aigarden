//! `file-length`: flag files over their per-glob size budget.
//!
//! The metric is per budget group because "length" means different things — code
//! is budgeted in readable **lines**, always-loaded guidance and prose in context
//! **tokens** (`ceil(chars/4)`). Both counts read the raw content so a missing
//! trailing newline can't undercount (the `wc -l` bug this replaces).

use globset::{Glob, GlobMatcher};
use rayon::prelude::*;

use crate::config::{Budget, Metric};
use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub(crate) struct FileLength;

impl Rule for FileLength {
    fn name(&self) -> &'static str {
        "file-length"
    }

    fn description(&self) -> &'static str {
        "flag files that exceed their per-glob size budget (lines or tokens)"
    }

    fn enabled(&self, config: &crate::config::Config) -> bool {
        config.file_length.enabled
    }

    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        // Compile each budget's glob once, preserving first-match order.
        let budgets: Vec<CompiledBudget> = ctx
            .config
            .file_length
            .effective_budgets()
            .into_iter()
            .map(CompiledBudget::compile)
            .collect::<Result<_, _>>()
            .expect("config budget globs validated at load");

        ctx.files
            .par_iter()
            .filter_map(|file| {
                let budget = budgets
                    .iter()
                    .find(|b| b.matcher.is_match(&file.rel_path))?;
                let value = measure(&file.content, budget.metric);
                (value > budget.max).then(|| Diagnostic {
                    rule: "file-length",
                    path: file.rel_path.clone(),
                    span: None,
                    message: format!(
                        "{value} {metric} exceeds the budget of {max} for `{glob}`",
                        metric = budget.metric.as_str(),
                        max = budget.max,
                        glob = budget.glob,
                    ),
                    suggestion: Some(format!(
                        "split the file or raise the `{}` budget in ailint.toml",
                        budget.glob
                    )),
                })
            })
            .collect()
    }
}

struct CompiledBudget {
    matcher: GlobMatcher,
    metric: Metric,
    max: usize,
    glob: String,
}

impl CompiledBudget {
    fn compile(budget: Budget) -> Result<Self, globset::Error> {
        Ok(Self {
            matcher: Glob::new(&budget.glob)?.compile_matcher(),
            metric: budget.metric,
            max: budget.max,
            glob: budget.glob,
        })
    }
}

fn measure(content: &str, metric: Metric) -> usize {
    match metric {
        Metric::Lines => line_count(content),
        Metric::Tokens => token_count(content),
    }
}

/// True line count: newlines, plus one for a final line with no trailing `\n`.
fn line_count(content: &str) -> usize {
    if content.is_empty() {
        return 0;
    }
    let newlines = content.matches('\n').count();
    if content.ends_with('\n') {
        newlines
    } else {
        newlines + 1
    }
}

/// `ceil(chars / 4)` over Unicode scalar values of the raw content.
fn token_count(content: &str) -> usize {
    content.chars().count().div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_count_is_trailing_newline_insensitive() {
        // The bug this rule fixes: "a\nb" is 2 lines even without a final newline.
        assert_eq!(line_count("a\nb\n"), 2);
        assert_eq!(line_count("a\nb"), 2);
        assert_eq!(line_count("a\n"), 1);
        assert_eq!(line_count(""), 0);
    }

    #[test]
    fn token_count_rounds_up_from_chars() {
        assert_eq!(token_count(""), 0);
        assert_eq!(token_count("abc"), 1); // ceil(3/4)
        assert_eq!(token_count("abcd"), 1); // ceil(4/4)
        assert_eq!(token_count("abcde"), 2); // ceil(5/4)
    }
}
