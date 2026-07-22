//! The shared reference-extraction core: the one place that finds every
//! checkable reference in a file, with byte spans, **decoupled from validation**.
//!
//! This is the load-bearing abstraction of the tool. Both `check` (whose link
//! rules validate references) and `mv` (which rewrites them) read from this one
//! module, so their link grammars cannot drift apart — the class of bug the
//! hand-grown tooling this replaces kept re-introducing by keeping two regex
//! copies in sync by hand.
//!
//! Extraction answers only "what references does this file contain, and where?"
//! It never touches the filesystem and never decides whether a target is valid —
//! that is a rule's job. A [`Reference`] carries enough structure (`path`,
//! `fragment`, and the byte span of the raw target) for a validator to resolve it
//! and for `mv` to rewrite it in place.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// One checkable reference found in a file. Renderer- and validator-agnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Which grammar produced this reference.
    pub kind: RefKind,
    /// The whole target exactly as written, e.g. `docs/guide.md#anchors`,
    /// `../a/pic.png`, `@CONTEXT_SHORT.md`, or a bare `#section`. For an
    /// `@`-import the leading `@` is **not** included.
    pub raw_target: String,
    /// The path portion of `raw_target` (target minus `#fragment`), or `None`
    /// for a pure same-file anchor like `#section`.
    pub path: Option<String>,
    /// The `#fragment` portion without its leading `#`, if any.
    pub fragment: Option<String>,
    /// Byte span of `raw_target` within the file — the exact region `mv`
    /// rewrites. Rewriting the path while preserving a fragment is
    /// `new_path + fragment.map(|f| format!("#{f}"))`.
    pub target_span: Range<usize>,
}

/// The reference grammars aigarden recognizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// A markdown inline or reference-definition link `[text](target)`.
    MarkdownLink,
    /// A markdown image `![alt](target)`.
    MarkdownImage,
    /// An `@path` import in an always-loaded guidance file.
    AtImport,
    /// A backticked, file-shaped path in markdown prose.
    BarePath,
    /// A root-relative doc path cited inside a non-markdown source file.
    CodeDocRef,
}

/// Every reference in `content`, chosen by `rel_path`'s file type: markdown files
/// yield links, images, bare paths, and (in a guidance file) `@`-imports;
/// non-markdown files yield code doc-path citations.
///
/// This is the single entry point rules and `mv` share. Callers filter by
/// [`RefKind`]; ordering is by byte span so output is deterministic.
#[must_use]
pub fn extract(rel_path: &str, content: &str) -> Vec<Reference> {
    let mut refs = if is_markdown(rel_path) {
        let mut r = extract_markdown(content);
        if is_guidance(rel_path) {
            r.extend(extract_at_imports(content));
        }
        r
    } else {
        extract_code_doc_refs(content)
    };
    refs.sort_by_key(|r| r.target_span.start);
    refs
}

/// True for markdown files (the set the link/anchor/bare-path rules scan).
#[must_use]
pub fn is_markdown(rel_path: &str) -> bool {
    matches!(
        extension(rel_path).as_deref(),
        Some("md" | "markdown" | "mdx")
    )
}

/// True for always-loaded guidance files, where `@`-imports are live and fail
/// silently at runtime if broken. Matched on basename in any directory.
#[must_use]
pub fn is_guidance(rel_path: &str) -> bool {
    let base = rel_path.rsplit('/').next().unwrap_or(rel_path);
    matches!(base, "CLAUDE.md" | "AGENTS.md" | "SKILL.md" | "GEMINI.md")
}

/// Lowercased final extension of a path, if any.
fn extension(rel_path: &str) -> Option<String> {
    let base = rel_path.rsplit('/').next().unwrap_or(rel_path);
    base.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

/// Split a raw target into its path and fragment parts. A leading `#` means a
/// pure same-file anchor (path `None`); otherwise the fragment is whatever
/// follows the first `#`.
fn split_fragment(raw: &str) -> (Option<String>, Option<String>) {
    match raw.split_once('#') {
        Some(("", frag)) => (None, Some(frag.to_string())),
        Some((path, frag)) => (Some(path.to_string()), Some(frag.to_string())),
        None => (Some(raw.to_string()), None),
    }
}

/// Markdown links and images via pulldown-cmark, plus reference-style link
/// definitions (whose rewritable target lives on the definition line, not the
/// usage site). Bare backticked paths are collected in the same AST walk so the
/// code-span vs prose distinction is the parser's, not a regex's.
fn extract_markdown(content: &str) -> Vec<Reference> {
    let mut refs = Vec::new();
    // A backticked code span that is a link's *text* (`[`a/b.md`](url)`) is a link
    // label, not a bare path — its target is the link rules' job. Track link/image
    // nesting so those inner code spans are skipped (mirrors the source tool's
    // `]( ` guard).
    let mut link_depth = 0usize;
    let parser = Parser::new_ext(content, Options::all()).into_offset_iter();
    for (event, range) in parser {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                link_depth += 1;
                push_dest(&mut refs, RefKind::MarkdownLink, &dest_url, content, &range);
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                link_depth += 1;
                push_dest(
                    &mut refs,
                    RefKind::MarkdownImage,
                    &dest_url,
                    content,
                    &range,
                );
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                link_depth = link_depth.saturating_sub(1);
            }
            Event::Code(code) if link_depth == 0 => {
                if let Some(reference) = bare_path_ref(&code, &range) {
                    refs.push(reference);
                }
            }
            _ => {}
        }
    }
    // Reference-style definitions: `[id]: dest "title"` — the target `mv` rewrites.
    // Re-parse for definitions (the offset iter above resolves usages to their dest
    // but reports the usage span, which is the wrong region to rewrite).
    let parser = Parser::new_ext(content, Options::all());
    for (_label, def) in parser.reference_definitions().iter() {
        push_dest(
            &mut refs,
            RefKind::MarkdownLink,
            &def.dest,
            content,
            &def.span,
        );
    }
    refs
}

/// Locate `dest` within a link/image/definition element `range` and push a
/// [`Reference`] spanning exactly the destination text. The destination follows
/// the `](` of an inline link or the `]:` of a reference definition; searching
/// from that boundary avoids matching an identical substring in the link text.
fn push_dest(
    refs: &mut Vec<Reference>,
    kind: RefKind,
    dest: &str,
    content: &str,
    range: &Range<usize>,
) {
    if dest.is_empty() {
        return;
    }
    let element = &content[range.clone()];
    // Inline links use `](`; reference definitions use `]:`. Take whichever the
    // element contains, then find the destination after it.
    let boundary = element
        .find("](")
        .map(|i| i + 2)
        .or_else(|| element.find("]:").map(|i| i + 2))
        .unwrap_or(0);
    let Some(rel) = element[boundary..].find(dest) else {
        return;
    };
    let start = range.start + boundary + rel;
    let target_span = start..start + dest.len();
    let (path, fragment) = split_fragment(dest);
    refs.push(Reference {
        kind,
        raw_target: dest.to_string(),
        path,
        fragment,
        target_span,
    });
}

/// A backticked code span becomes a bare-path reference iff it *looks like* a
/// file path: an interior slash (2+ segments), no `..`, and a real-looking
/// extension on the last segment. This keeps prose backticks (`foo_bar`,
/// `SomeType`, `--flag`) out while catching `docs/x.py`-shaped citations. The
/// existence decision is the rule's; this only recognizes the shape.
fn bare_path_ref(code: &str, range: &Range<usize>) -> Option<Reference> {
    let text = code.trim();
    if !looks_like_path(text) {
        return None;
    }
    // The code span range includes the surrounding backticks; point at the text.
    let start = range.start + 1;
    let (path, fragment) = split_fragment(text);
    Some(Reference {
        kind: RefKind::BarePath,
        raw_target: text.to_string(),
        path,
        fragment,
        target_span: start..start + text.len(),
    })
}

/// Characters that mark a backticked span as code/glob/template rather than a
/// path — a span containing any of these is prose, not a reference to resolve.
/// Mirrors the source tool's `_BAD_SPAN_CHARS`.
const BAD_SPAN_CHARS: &[char] = &[
    '(', ')', '{', '}', '<', '>', '*', '$', '=', ':', '@', '"', '\'', '\\',
];

/// Heuristic for "this backticked span is a file path, not prose".
fn looks_like_path(text: &str) -> bool {
    if text.is_empty()
        || text.contains(char::is_whitespace)
        || text.contains(BAD_SPAN_CHARS)
        || text.contains("..")
        || text.contains("NNNN") // a zero-padded-id placeholder, not a real path
        || text.starts_with(".git/") // a runtime artifact (hook symlinks), not tracked content
        || text.split('/').count() < 2
    {
        return false;
    }
    // Reject obvious non-paths that still contain a slash (e.g. `a/b` operators,
    // `and/or` prose): require a file-like extension on the final segment.
    let last = text.rsplit('/').next().unwrap_or(text);
    match last.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 5
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

/// `@path` imports: `@` followed by a file-shaped token, one per match. Generalized
/// beyond the source tool's "must contain a slash" rule (which misses a real
/// `@CONTEXT_SHORT.md` import): a token counts when it contains a slash **or**
/// carries a doc extension, which excludes decorator prose like `@workflow.defn`.
/// Trailing sentence punctuation is stripped.
fn extract_at_imports(content: &str) -> Vec<Reference> {
    let mut refs = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        // An `@` starts an import only at a word boundary (not mid-token like an
        // email `user@host` — the char before must not be an identifier char).
        let preceded_by_word = i > 0 && {
            let prev = content[..i].chars().next_back().unwrap();
            prev.is_alphanumeric() || prev == '_' || prev == '.' || prev == '/'
        };
        let token_start = i + 1;
        let token_end = token_start
            + content[token_start..]
                .find(|c: char| !is_import_char(c))
                .unwrap_or(content.len() - token_start);
        let mut token = &content[token_start..token_end];
        // Strip trailing sentence punctuation (`.`, `,`, `:`, `)`).
        token = token.trim_end_matches(['.', ',', ':', ')']);
        if !preceded_by_word && !token.is_empty() && is_import_shaped(token) {
            let (path, fragment) = split_fragment(token);
            refs.push(Reference {
                kind: RefKind::AtImport,
                raw_target: token.to_string(),
                path,
                fragment,
                target_span: token_start..token_start + token.len(),
            });
        }
        i = token_end.max(i + 1);
    }
    refs
}

fn is_import_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '-')
}

/// An `@`-token is import-shaped when it has an interior slash or a doc-file
/// extension. This admits `@docs/x.md` and `@CONTEXT_SHORT.md` while rejecting
/// `@workflow.defn` (`.defn` is not a doc extension) and bare `@mention`s.
fn is_import_shaped(token: &str) -> bool {
    if token.contains('/') {
        return true;
    }
    matches!(
        token.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()),
        Some(ref e) if matches!(e.as_str(), "md" | "markdown" | "mdx" | "txt")
    )
}

/// Root-relative doc-path citations in a non-markdown file: tokens like
/// `docs/…`, `issues/…`, `plans/…` ending in a file extension, inside comments or
/// strings. Language-agnostic (textual), matching the source tool's grammar;
/// resolution is root-relative only (a code file has no established doc dir).
fn extract_code_doc_refs(content: &str) -> Vec<Reference> {
    const ROOTS: [&str; 3] = ["docs/", "issues/", "plans/"];
    let mut refs = Vec::new();
    for root in ROOTS {
        let mut search_from = 0;
        while let Some(rel) = content[search_from..].find(root) {
            let start = search_from + rel;
            // Require a token boundary before the root so `mydocs/x` doesn't match.
            let boundary_ok = start == 0 || {
                let prev = content[..start].chars().next_back().unwrap();
                !(prev.is_alphanumeric() || prev == '_' || prev == '/' || prev == '.')
            };
            let end = start
                + content[start..]
                    .find(|c: char| !is_doc_ref_char(c))
                    .unwrap_or(content.len() - start);
            let token = content[start..end].trim_end_matches(['.', ',', ':']);
            if boundary_ok && has_extension(token) {
                refs.push(Reference {
                    kind: RefKind::CodeDocRef,
                    raw_target: token.to_string(),
                    path: Some(token.to_string()),
                    fragment: None,
                    target_span: start..start + token.len(),
                });
            }
            search_from = end.max(start + 1);
        }
    }
    refs.sort_by_key(|r| r.target_span.start);
    refs
}

fn is_doc_ref_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '-')
}

/// A path token ends in a `.ext` on its final segment (not a bare directory).
fn has_extension(token: &str) -> bool {
    let last = token.rsplit('/').next().unwrap_or(token);
    last.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && !ext.is_empty() && ext.len() <= 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A helper asserting the `kind`/`raw_target` of a reference and that its
    /// span slices back to exactly the raw target.
    fn assert_ref(r: &Reference, content: &str, kind: RefKind, raw: &str) {
        assert_eq!(r.kind, kind, "kind for {raw:?}");
        assert_eq!(r.raw_target, raw, "raw_target");
        assert_eq!(
            &content[r.target_span.clone()],
            raw,
            "span slices to target"
        );
    }

    #[test]
    fn extracts_inline_link_with_path_and_fragment() {
        let md = "See [the guide](docs/guide.md#anchors) now.\n";
        let refs = extract("README.md", md);
        assert_eq!(refs.len(), 1);
        assert_ref(&refs[0], md, RefKind::MarkdownLink, "docs/guide.md#anchors");
        assert_eq!(refs[0].path.as_deref(), Some("docs/guide.md"));
        assert_eq!(refs[0].fragment.as_deref(), Some("anchors"));
    }

    #[test]
    fn extracts_image_target() {
        let md = "![a picture](../assets/pic.png)\n";
        let refs = extract("doc.md", md);
        assert_eq!(refs.len(), 1);
        assert_ref(&refs[0], md, RefKind::MarkdownImage, "../assets/pic.png");
    }

    #[test]
    fn pure_anchor_link_has_no_path() {
        let md = "Jump to [the section](#my-heading).\n";
        let refs = extract("doc.md", md);
        assert_eq!(refs[0].path, None);
        assert_eq!(refs[0].fragment.as_deref(), Some("my-heading"));
    }

    #[test]
    fn reference_style_definition_is_extracted_not_usage() {
        // The rewritable target lives on the definition line, so the span must
        // point there — never at the `[text][g]` usage.
        let md = "Use [the guide][g] here.\n\n[g]: docs/guide.md#sec \"Title\"\n";
        let refs = extract("doc.md", md);
        let link = refs
            .iter()
            .find(|r| r.kind == RefKind::MarkdownLink)
            .unwrap();
        assert_ref(link, md, RefKind::MarkdownLink, "docs/guide.md#sec");
        // The span is on the definition line, not the usage.
        assert!(link.target_span.start > md.find("[g]:").unwrap());
    }

    #[test]
    fn backticked_file_path_is_a_bare_path_ref() {
        let md = "Run the linter in `tools/lint_links.py` please.\n";
        let refs = extract("doc.md", md);
        assert_eq!(refs.len(), 1);
        assert_ref(&refs[0], md, RefKind::BarePath, "tools/lint_links.py");
    }

    #[test]
    fn prose_backticks_are_not_bare_paths() {
        // No slash, or slash without a file extension, or a `..` — all prose.
        let md = "`SomeType`, `and/or`, `a/b`, `../up`, and `--flag` are not paths.\n";
        let refs = extract("doc.md", md);
        assert!(refs.is_empty(), "got {refs:?}");
    }

    #[test]
    fn bare_path_rejects_glob_template_and_placeholder_spans() {
        // Backticked spans carrying glob/template metacharacters, an `NNNN`
        // placeholder, or a `.git/` runtime-artifact prefix are not file paths to
        // resolve — mirrors the source tool's `_BAD_SPAN_CHARS`/`NNNN`/`.git/` guards.
        for span in [
            "`.claude/agents/*.md`",         // glob star
            "`handoffs/handoff-<topic>.md`", // template angle brackets
            "`issues/NNNN-title.md`",        // NNNN placeholder
            "`.git/hooks/pre-commit`",       // .git runtime artifact
            "`a/b(c).md`",                   // parens
        ] {
            let md = format!("Prose with {span} inline.\n");
            let refs = extract("doc.md", &md);
            assert!(
                refs.iter().all(|r| r.kind != RefKind::BarePath),
                "{span} should not be a bare path; got {refs:?}"
            );
        }
    }

    #[test]
    fn backticked_link_label_is_not_a_bare_path() {
        // A backticked code span that is a markdown link's *text* is the link's job
        // (link-target / link-case), never a bare path — mirrors the source tool's
        // `]( ` guard. The path-shaped label must not be flagged, but the link
        // target is still extracted.
        let md = "See [`triggerdotdev/trigger.dev`](https://example.com/x).\n";
        let refs = extract("doc.md", md);
        assert!(
            refs.iter().all(|r| r.kind != RefKind::BarePath),
            "got {refs:?}"
        );
        assert!(refs.iter().any(|r| r.kind == RefKind::MarkdownLink));
    }

    #[test]
    fn at_import_with_slash_and_without() {
        // Both a slash-bearing import and a bare doc-extension import resolve —
        // the latter is the case the source tool's slash-only rule missed.
        let md = "@docs/product_overview.md\n\n@CONTEXT_SHORT.md\n";
        let refs = extract("CLAUDE.md", md);
        let imports: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == RefKind::AtImport)
            .collect();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].raw_target, "docs/product_overview.md");
        assert_eq!(imports[1].raw_target, "CONTEXT_SHORT.md");
    }

    #[test]
    fn at_import_ignores_decorator_prose_and_emails() {
        let md = "The `@workflow.defn` decorator and me@example.com are not imports.\n";
        let refs = extract("CLAUDE.md", md);
        // `@workflow.defn` is code-span prose (not import-shaped: `.defn`), and the
        // email `@example.com` is preceded by a word char.
        assert!(
            refs.iter().all(|r| r.kind != RefKind::AtImport),
            "got {refs:?}"
        );
    }

    #[test]
    fn at_imports_only_extracted_in_guidance_files() {
        let md = "@docs/product_overview.md\n";
        assert!(extract("README.md", md).is_empty());
        assert_eq!(extract("AGENTS.md", md).len(), 1);
    }

    #[test]
    fn code_doc_ref_in_a_non_markdown_file() {
        let src = "// see docs/design.md for the rationale\nfn main() {}\n";
        let refs = extract("src/main.rs", src);
        assert_eq!(refs.len(), 1);
        assert_ref(&refs[0], src, RefKind::CodeDocRef, "docs/design.md");
    }

    #[test]
    fn code_doc_ref_requires_a_token_boundary_and_extension() {
        // `mydocs/x.md` is not a root ref; `docs/adrs` (no extension) is a bare dir.
        let src = "let a = \"mydocs/x.md\"; let b = \"docs/adrs\";\n";
        assert!(extract("a.rs", src).is_empty());
    }

    #[test]
    fn markdown_files_do_not_yield_code_doc_refs() {
        // In markdown, `docs/x.md` outside a link/backtick is just prose.
        let md = "Plain prose mentioning docs/design.md inline.\n";
        let refs = extract("doc.md", md);
        assert!(refs.iter().all(|r| r.kind != RefKind::CodeDocRef));
    }
}
