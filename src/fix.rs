//! `--fix`: apply the auto-fixable `markdown-style` rules to disk, in place.
//!
//! Only the curated rumdl style rules are fixable; the reference-integrity rules
//! (a broken link, a missing anchor) need a human decision and are never touched.
//! Fixes are chained rule-by-rule per file, so each rule fixes on the previous
//! rule's output — the same order `rumdl --fix` uses. A fix never writes inside a
//! cog block: its generator owns those bytes (see [`crate::rumdl_adapter::CogLayout`]).
//! After a fix pass the caller re-runs the check, so any residue (e.g. a line
//! reflow cannot shorten, or a generated defect) is still reported, and a second
//! `--fix` run is a clean no-op.

use anyhow::{Context, Result};

use crate::config::Resolver;
use crate::references::is_markdown;
use crate::rumdl_adapter::{fix, style_rules};
use crate::walk::SourceFile;

/// Rewrite each markdown file with the style fixes applied, updating `files` in
/// place so the subsequent check sees the fixed content. `reflow` is a global
/// option, and a file disabled via `ignore` / `[per-file-ignores]` is skipped — so
/// the fix pass and the check agree on every file.
pub(crate) fn apply(files: &mut [SourceFile], resolver: &Resolver<'_>) -> Result<()> {
    let rules = style_rules(resolver.markdown_style().reflow);
    for file in files
        .iter_mut()
        .filter(|f| is_markdown(&f.rel_path) && resolver.is_enabled("markdown-style", &f.rel_path))
    {
        let content = fix(&file.content, &file.abs_path, &rules)
            .with_context(|| format!("fixing {}", file.rel_path))?;
        if content != file.content {
            std::fs::write(&file.abs_path, &content)
                .with_context(|| format!("writing fixes to {}", file.rel_path))?;
            file.content = content;
        }
    }
    Ok(())
}
