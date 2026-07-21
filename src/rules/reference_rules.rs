//! The reference-integrity rules that read from the shared extraction core and
//! validate against the filesystem. Each is a thin, deep rule: pull references of
//! one kind out of [`crate::references`], resolve them via [`super::resolve`], and
//! report every target that does not exist (or, for `link-case`, exists under a
//! different case).
//!
//! Splitting extraction (one grammar) from validation (these rules) is the design
//! this tool is built on — the same references feed a future `mv` unchanged.

use crate::diagnostic::{Diagnostic, Span};
use crate::references::{RefKind, extract, is_markdown};
use crate::rules::resolve::{
    case_exact, is_checkable_local, is_gitignored, resolve_existing, resolve_from_file,
    resolve_from_root,
};
use crate::rules::{ENABLED_ONLY, Explanation, Rule, RuleContext};
use crate::walk::SourceFile;

/// Build a spanned diagnostic for a reference finding.
fn finding(
    rule: &'static str,
    file: &SourceFile,
    span: std::ops::Range<usize>,
    message: String,
    suggestion: Option<String>,
) -> Diagnostic {
    Diagnostic {
        rule,
        path: file.rel_path.clone(),
        span: Some(Span::from_byte_range(&file.content, span)),
        message,
        suggestion,
    }
}

/// `link-target`: a relative markdown link or image points at a file on disk.
/// Extensionless wiki-style targets also try a `.md` sibling.
pub(crate) struct LinkTarget;

impl Rule for LinkTarget {
    fn name(&self) -> &'static str {
        "link-target"
    }
    fn description(&self) -> &'static str {
        "a relative markdown link or image target exists on disk"
    }
    fn explain(&self) -> Explanation {
        Explanation {
            checks: "A relative markdown link or image target resolves to a file on disk, from \
the linking file's own directory. An extensionless wiki-style target also tries a `.md` \
sibling. External URLs, absolute paths, and pure `#anchors` are left alone.",
            config: ENABLED_ONLY,
            example: "link target `../guide.md` does not exist",
            fix: None,
            config_gated: false,
        }
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for file in ctx
            .files
            .iter()
            .filter(|f| is_markdown(&f.rel_path) && ctx.resolver.link_target(&f.rel_path))
        {
            for reference in extract(&file.rel_path, &file.content) {
                if !matches!(
                    reference.kind,
                    RefKind::MarkdownLink | RefKind::MarkdownImage
                ) {
                    continue;
                }
                let Some(path) = reference.path.as_deref().filter(|p| is_checkable_local(p)) else {
                    continue;
                };
                let resolved = resolve_from_file(&file.abs_path, path);
                if resolve_existing(&resolved).is_none() {
                    diagnostics.push(finding(
                        self.name(),
                        file,
                        reference.target_span,
                        format!("link target `{path}` does not exist"),
                        Some("fix the path, or create the target".to_string()),
                    ));
                }
            }
        }
        diagnostics
    }
}

/// `link-case`: a link target that exists but under a different case — the macOS
/// case-insensitivity trap that 404s on case-sensitive CI. Only flags existing
/// targets, so a true 404 is `link-target`'s alone (no double report).
pub(crate) struct LinkCase;

impl Rule for LinkCase {
    fn name(&self) -> &'static str {
        "link-case"
    }
    fn description(&self) -> &'static str {
        "a link target's case matches the filesystem exactly"
    }
    fn explain(&self) -> Explanation {
        Explanation {
            checks: "A link target that exists but under a different case — the macOS \
case-insensitivity trap that opens locally yet 404s on case-sensitive CI. Only existing \
targets are flagged, so a genuine 404 stays link-target's alone (no double report).",
            config: ENABLED_ONLY,
            example: "link target `docs/Guide.md` case does not match the file on disk",
            fix: None,
            config_gated: false,
        }
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for file in ctx.files.iter().filter(|f| {
            is_markdown(&f.rel_path)
                && ctx.resolver.link_case(&f.rel_path)
                // Frozen (terminal-status) docs may cite old-cased historical paths.
                && !ctx.frozen_suppressed(self.name(), &f.rel_path)
        }) {
            for reference in extract(&file.rel_path, &file.content) {
                if !matches!(
                    reference.kind,
                    RefKind::MarkdownLink | RefKind::MarkdownImage
                ) {
                    continue;
                }
                let Some(path) = reference.path.as_deref().filter(|p| is_checkable_local(p)) else {
                    continue;
                };
                let resolved = resolve_from_file(&file.abs_path, path);
                if let Some(existing) = resolve_existing(&resolved)
                    && !case_exact(&existing)
                {
                    diagnostics.push(finding(
                        self.name(),
                        file,
                        reference.target_span,
                        format!("link target `{path}` case does not match the file on disk"),
                        Some("match the committed path's exact case".to_string()),
                    ));
                }
            }
        }
        diagnostics
    }
}

/// `bare-path`: a backticked file-shaped path in prose exists, resolved against
/// the file's own directory or the repo root.
pub(crate) struct BarePath;

impl Rule for BarePath {
    fn name(&self) -> &'static str {
        "bare-path"
    }
    fn description(&self) -> &'static str {
        "a backticked file-shaped path in markdown prose exists"
    }
    fn explain(&self) -> Explanation {
        Explanation {
            checks: "A backticked, file-shaped path in markdown prose exists, resolved against \
the file's own directory or the repo root. Shell/glob metacharacters, `NNNN` placeholders, and \
markdown-link labels are not treated as paths; a candidate resolving to a gitignored path is \
skipped as an environment artifact.",
            config: ENABLED_ONLY,
            example: "bare path `src/missing.rs` does not exist",
            fix: None,
            config_gated: false,
        }
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let gitignore = crate::walk::root_gitignore(ctx.root);
        let mut diagnostics = Vec::new();
        for file in ctx.files.iter().filter(|f| {
            is_markdown(&f.rel_path)
                && ctx.resolver.bare_path(&f.rel_path)
                // Frozen (terminal-status) docs may cite now-gone historical paths.
                && !ctx.frozen_suppressed(self.name(), &f.rel_path)
        }) {
            for reference in extract(&file.rel_path, &file.content) {
                if reference.kind != RefKind::BarePath {
                    continue;
                }
                let Some(path) = reference.path.as_deref() else {
                    continue;
                };
                let from_file = resolve_from_file(&file.abs_path, path);
                let from_root = resolve_from_root(ctx.root, path);
                // A candidate resolving to a gitignored path is an environment
                // artifact (generated locally, absent on a fresh checkout) — skip it.
                if is_gitignored(&gitignore, ctx.root, &from_file)
                    || is_gitignored(&gitignore, ctx.root, &from_root)
                {
                    continue;
                }
                if resolve_existing(&from_file).is_none() && resolve_existing(&from_root).is_none()
                {
                    diagnostics.push(finding(
                        self.name(),
                        file,
                        reference.target_span,
                        format!("bare path `{path}` does not exist"),
                        None,
                    ));
                }
            }
        }
        diagnostics
    }
}

/// `import-target`: an `@path` import in an always-loaded guidance file resolves.
/// These fail silently at runtime, so nothing else catches a broken one.
pub(crate) struct ImportTarget;

impl Rule for ImportTarget {
    fn name(&self) -> &'static str {
        "import-target"
    }
    fn description(&self) -> &'static str {
        "an `@path` import in a guidance file resolves on disk"
    }
    fn explain(&self) -> Explanation {
        Explanation {
            checks: "An `@path` import in an always-loaded guidance file (e.g. CLAUDE.md) \
resolves on disk. A broken `@`-import fails silently at load time, so nothing else catches it.",
            config: ENABLED_ONLY,
            example: "import `@docs/gone.md` does not resolve",
            fix: None,
            config_gated: false,
        }
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for file in ctx
            .files
            .iter()
            .filter(|f| is_markdown(&f.rel_path) && ctx.resolver.import_target(&f.rel_path))
        {
            for reference in extract(&file.rel_path, &file.content) {
                if reference.kind != RefKind::AtImport {
                    continue;
                }
                let Some(path) = reference.path.as_deref() else {
                    continue;
                };
                let resolved = resolve_from_file(&file.abs_path, path);
                if resolve_existing(&resolved).is_none() {
                    diagnostics.push(finding(
                        self.name(),
                        file,
                        reference.target_span,
                        format!("import `@{path}` does not resolve"),
                        Some("a broken @-import fails silently at load time".to_string()),
                    ));
                }
            }
        }
        diagnostics
    }
}

/// `code-doc-ref`: a doc path (`docs/…`, `issues/…`, `plans/…`) cited inside a
/// non-markdown source file exists. Root-relative only — nothing establishes a
/// code file's doc directory.
pub(crate) struct CodeDocRef;

impl Rule for CodeDocRef {
    fn name(&self) -> &'static str {
        "code-doc-ref"
    }
    fn description(&self) -> &'static str {
        "a doc path cited inside a non-markdown source file exists"
    }
    fn explain(&self) -> Explanation {
        Explanation {
            checks: "A doc path (`docs/…`, `issues/…`, `plans/…`) cited inside a non-markdown \
source file exists, resolved repo-root-relative (nothing establishes a code file's doc \
directory). A candidate resolving to a gitignored path is skipped as an environment artifact.",
            config: ENABLED_ONLY,
            example: "doc path `docs/gone.md` cited here does not exist",
            fix: None,
            config_gated: false,
        }
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let gitignore = crate::walk::root_gitignore(ctx.root);
        let mut diagnostics = Vec::new();
        for file in ctx
            .files
            .iter()
            .filter(|f| !is_markdown(&f.rel_path) && ctx.resolver.code_doc_ref(&f.rel_path))
        {
            for reference in extract(&file.rel_path, &file.content) {
                if reference.kind != RefKind::CodeDocRef {
                    continue;
                }
                let Some(path) = reference.path.as_deref() else {
                    continue;
                };
                let resolved = resolve_from_root(ctx.root, path);
                // Skip a candidate resolving to a gitignored path (environment
                // artifact) — the same rule bare-path applies.
                if is_gitignored(&gitignore, ctx.root, &resolved) {
                    continue;
                }
                if resolve_existing(&resolved).is_none() {
                    diagnostics.push(finding(
                        self.name(),
                        file,
                        reference.target_span,
                        format!("doc path `{path}` cited here does not exist"),
                        None,
                    ));
                }
            }
        }
        diagnostics
    }
}
