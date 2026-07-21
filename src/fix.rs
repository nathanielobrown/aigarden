//! `--fix`: apply the auto-fixable `markdown-style` rules to disk, in place.
//!
//! Only the curated rumdl style rules are fixable; the reference-integrity rules
//! (a broken link, a missing anchor) need a human decision and are never touched.
//! Fixes are chained rule-by-rule per file, so each rule fixes on the previous
//! rule's output — the same order `rumdl --fix` uses. After a fix pass the caller
//! re-runs the check, so any residue (e.g. a line reflow cannot shorten) is still
//! reported, and a second `--fix` run is a clean no-op.

use anyhow::{Context, Result, anyhow};
use rumdl_lib::LintContext;
use rumdl_lib::config::MarkdownFlavor;

use crate::config::Resolver;
use crate::references::is_markdown;
use crate::rumdl_adapter::style_rules;
use crate::walk::SourceFile;

/// Rewrite each markdown file with the style fixes applied, updating `files` in
/// place so the subsequent check sees the fixed content. Each file's `markdown-style`
/// config is resolved through `resolver`, so an override can skip a file or change
/// its `reflow` — the fix pass and the check agree on every file.
pub(crate) fn apply(files: &mut [SourceFile], resolver: &Resolver<'_>) -> Result<()> {
    for file in files.iter_mut().filter(|f| is_markdown(&f.rel_path)) {
        let cfg = resolver.markdown_style(&file.rel_path);
        if !cfg.enabled {
            continue;
        }
        let rules = style_rules(cfg.reflow);
        let mut content = file.content.clone();
        for rule in &rules {
            let ctx = LintContext::new(
                &content,
                MarkdownFlavor::Standard,
                Some(file.abs_path.clone()),
            );
            content = rule
                .fix(&ctx)
                .map_err(|e| anyhow!("fixing {} with {}: {e}", file.rel_path, rule.name()))?;
        }
        if content != file.content {
            std::fs::write(&file.abs_path, &content)
                .with_context(|| format!("writing fixes to {}", file.rel_path))?;
            file.content = content;
        }
    }
    Ok(())
}
