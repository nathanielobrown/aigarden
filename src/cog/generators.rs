//! The built-in cog generators plus the embedded-shell escape hatch.
//!
//! Each generator is a pure function of the repository on disk: given its
//! arguments, the cog file's absolute path, and the file's repo root, it returns
//! the text to splice between the markers. Output is deterministic — the same
//! tree yields byte-identical output — so a `--check` right after a `--write` is
//! always clean.
//!
//! The three built-ins are a deliberately non-Turing-complete template language;
//! `sh` is the escape hatch for everything they do not cover.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use globset::Glob;
use ignore::WalkBuilder;

use crate::rules::resolve::relative_path;

/// Dispatch a generator by name. Unknown names and bad arguments fail loudly —
/// a broken cog directive is a config error, never a silent empty region.
pub(crate) fn generate(
    generator: &str,
    args: &str,
    file_abs: &Path,
    repo_root: &Path,
) -> Result<String> {
    match generator {
        "file-tree" => file_tree(args, file_abs),
        "first-sentences" => first_sentences(args, file_abs),
        "index" => index(args, file_abs, repo_root),
        "sh" => sh(args, repo_root),
        other => bail!("unknown cog generator `{other}`"),
    }
}

/// `file-tree <path>` — an indented directory tree of `<path>` (resolved from the
/// cog file's directory), honoring `.gitignore` and skipping hidden files and
/// `.git`. Directories carry a trailing `/`. Ordering is by path, so the output
/// is deterministic.
fn file_tree(args: &str, file_abs: &Path) -> Result<String> {
    let path = single_positional(args, "file-tree")?;
    let base = parent_dir(file_abs).join(&path);
    if !base.exists() {
        bail!("file-tree path `{path}` does not exist");
    }
    let mut entries: Vec<(String, usize, bool)> = Vec::new();
    for result in WalkBuilder::new(&base).build() {
        let entry = result.context("walking file-tree")?;
        if entry.depth() == 0 {
            continue; // the root itself
        }
        let rel = entry
            .path()
            .strip_prefix(&base)
            .context("file-tree entry outside its root")?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        entries.push((rel_str, entry.depth(), is_dir));
    }
    // Sort by path so a parent always precedes its children and siblings are
    // alphabetical — the stable order that makes the render deterministic.
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::new();
    for (rel_str, depth, is_dir) in entries {
        let name = rel_str.rsplit('/').next().unwrap_or(&rel_str);
        let indent = "  ".repeat(depth - 1);
        out.push_str(&indent);
        out.push_str(name);
        if is_dir {
            out.push('/');
        }
        out.push('\n');
    }
    Ok(out)
}

/// `first-sentences <path>` — the short-glossary projection. Reads a markdown file
/// of `## Section` headers and `- **Term** — gloss` bullets (resolved from the cog
/// file's directory) and emits each section with every bullet's gloss cut to its
/// first sentence. This is the `CONTEXT_SHORT.md` pattern.
fn first_sentences(args: &str, file_abs: &Path) -> Result<String> {
    let path = single_positional(args, "first-sentences")?;
    let target = parent_dir(file_abs).join(&path);
    let content = fs::read_to_string(&target)
        .with_context(|| format!("reading first-sentences target `{path}`"))?;
    let mut out = String::new();
    let mut first_section = true;
    for line in content.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if !first_section {
                out.push('\n');
            }
            first_section = false;
            out.push_str("## ");
            out.push_str(heading.trim());
            out.push_str("\n\n");
        } else if let Some((term, gloss)) = parse_term_bullet(line) {
            out.push_str("- **");
            out.push_str(term);
            // U+2014 em dash, the glossary separator.
            out.push_str("** \u{2014} ");
            out.push_str(&first_sentence(gloss));
            out.push('\n');
        }
    }
    Ok(out)
}

/// `index <glob>` — one bullet per file matching `<glob>` (relative to the repo
/// root), each linking the file (relative to the cog file) with a gloss taken
/// from a frontmatter `description`, else the first heading's following sentence.
fn index(args: &str, file_abs: &Path, repo_root: &Path) -> Result<String> {
    let pattern = single_positional(args, "index")?;
    let matcher = Glob::new(&pattern)
        .with_context(|| format!("invalid index glob `{pattern}`"))?
        .compile_matcher();
    let mut hits: Vec<std::path::PathBuf> = Vec::new();
    for result in WalkBuilder::new(repo_root).build() {
        let entry = result.context("walking for index")?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(repo_root)
            .context("index entry outside repo root")?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if matcher.is_match(&rel_str) {
            hits.push(entry.path().to_path_buf());
        }
    }
    hits.sort();
    let cog_dir = parent_dir(file_abs);
    let mut out = String::new();
    for path in hits {
        let content = fs::read_to_string(&path).unwrap_or_default();
        let stem = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let (title, gloss) = title_and_gloss(&content, &stem);
        let link = relative_path(cog_dir, &path);
        out.push_str("- [");
        out.push_str(&title);
        out.push_str("](");
        out.push_str(&link);
        out.push(')');
        if !gloss.is_empty() {
            out.push_str(" \u{2014} ");
            out.push_str(&gloss);
        }
        out.push('\n');
    }
    Ok(out)
}

/// `sh "<command>"` — run the command via `sh -c`, splice its stdout. Runs with
/// cwd = the file's repo root. A nonzero exit is a tool error with stderr
/// surfaced, never a silent empty region.
fn sh(args: &str, repo_root: &Path) -> Result<String> {
    let tokens = tokenize(args);
    let [command] = tokens.as_slice() else {
        bail!("`sh` needs exactly one quoted command: <!-- ailint:cog sh \"…\" -->");
    };
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("running cog shell command `{command}`"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        let detail = if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        };
        bail!(
            "cog shell command `{command}` exited with {}{detail}",
            output.status,
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The directory containing `file_abs`, or `file_abs` itself as a fallback.
fn parent_dir(file_abs: &Path) -> &Path {
    file_abs.parent().unwrap_or(file_abs)
}

/// Parse `- **Term** — gloss`, returning `(term, gloss)`. Requires the literal
/// ` — ` (em dash) separator the glossary convention uses.
fn parse_term_bullet(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start().strip_prefix("- **")?;
    let (term, after) = rest.split_once("**")?;
    let gloss = after.strip_prefix(" \u{2014} ")?;
    Some((term, gloss))
}

/// Cut `text` to its first sentence: the text up to the first top-level `.`
/// followed by whitespace or end. Periods inside backticks, parentheses, or
/// brackets, and the abbreviations `e.g.`/`i.e.`, do not end the sentence.
fn first_sentence(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut in_tick = false;
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '`' => in_tick = !in_tick,
            '(' if !in_tick => paren += 1,
            ')' if !in_tick => paren -= 1,
            '[' if !in_tick => bracket += 1,
            ']' if !in_tick => bracket -= 1,
            '.' if !in_tick && paren <= 0 && bracket <= 0 => {
                let ends = chars.get(i + 1).is_none_or(|n| n.is_whitespace());
                if ends {
                    let prefix: String = chars[..i].iter().collect();
                    // Don't cut mid-abbreviation.
                    if prefix.ends_with("e.g") || prefix.ends_with("i.e") {
                        continue;
                    }
                    return prefix.trim().to_string();
                }
            }
            _ => {}
        }
    }
    text.trim().to_string()
}

/// A title and a one-line gloss for an indexed file. Title is the first `#`
/// heading (else the filename stem); gloss is a frontmatter `description`, else
/// the first sentence of the first prose paragraph.
fn title_and_gloss(content: &str, stem: &str) -> (String, String) {
    let (description, body) = split_frontmatter(content);
    let mut title = None;
    let mut first_para = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("# ") {
            title.get_or_insert_with(|| heading.trim().to_string());
            continue;
        }
        if trimmed.starts_with('#') {
            continue; // deeper heading, not the title
        }
        if first_para.is_none() {
            first_para = Some(trimmed.to_string());
        }
    }
    let title = title.unwrap_or_else(|| stem.to_string());
    let gloss = description
        .or_else(|| first_para.map(|p| first_sentence(&p)))
        .unwrap_or_default();
    (title, gloss)
}

/// Split optional YAML frontmatter off the front of a markdown file, returning
/// any `description:` value and the remaining body.
fn split_frontmatter(content: &str) -> (Option<String>, &str) {
    let Some(rest) = content.strip_prefix("---\n") else {
        return (None, content);
    };
    let Some(end) = rest.find("\n---") else {
        return (None, content);
    };
    let frontmatter = &rest[..end];
    let mut description = None;
    for line in frontmatter.lines() {
        if let Some(value) = line.strip_prefix("description:") {
            description = Some(value.trim().trim_matches('"').to_string());
        }
    }
    // Body starts after the closing `\n---` line.
    let after = &rest[end + 4..];
    let body = after.strip_prefix('\n').unwrap_or(after);
    (description, body)
}

/// The single positional path argument a built-in expects, with no extra tokens.
fn single_positional(args: &str, generator: &str) -> Result<String> {
    let tokens = tokenize(args);
    match tokens.as_slice() {
        [path] => Ok(path.clone()),
        [] => bail!("`{generator}` needs a path argument"),
        _ => bail!("`{generator}` takes a single path argument, got {tokens:?}"),
    }
}

/// Split an argument string into tokens, treating a `"…"` run as one token
/// (so a shell command or a path with spaces stays intact).
fn tokenize(args: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut has_token = false;
    for c in args.chars() {
        if in_quote {
            if c == '"' {
                in_quote = false;
                tokens.push(std::mem::take(&mut current));
                has_token = false;
            } else {
                current.push(c);
            }
        } else if c == '"' {
            in_quote = true;
            has_token = true;
        } else if c.is_whitespace() {
            if has_token {
                tokens.push(std::mem::take(&mut current));
                has_token = false;
            }
        } else {
            current.push(c);
            has_token = true;
        }
    }
    if has_token {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sentence_cuts_at_top_level_period() {
        // A trailing sentence is dropped; a colon does not end the sentence.
        let s = "the unit of automation: a typed function. It orchestrates activities.";
        assert_eq!(
            first_sentence(s),
            "the unit of automation: a typed function"
        );
    }

    #[test]
    fn first_sentence_ignores_periods_inside_backticks_and_abbreviations() {
        assert_eq!(first_sentence("a `foo.bar` call"), "a `foo.bar` call");
        assert_eq!(
            first_sentence("e.g. this whole thing"),
            "e.g. this whole thing"
        );
    }

    #[test]
    fn tokenize_keeps_a_quoted_command_intact() {
        assert_eq!(tokenize("sh not-used"), vec!["sh", "not-used"]);
        assert_eq!(tokenize(r#""echo hi | wc -l""#), vec!["echo hi | wc -l"]);
    }

    #[test]
    fn parse_term_bullet_requires_the_em_dash_separator() {
        assert_eq!(
            parse_term_bullet("- **Workflow** \u{2014} the unit of automation"),
            Some(("Workflow", "the unit of automation"))
        );
        assert_eq!(
            parse_term_bullet("- **Workflow** - hyphen not em dash"),
            None
        );
        assert_eq!(parse_term_bullet("plain prose line"), None);
    }

    #[test]
    fn title_and_gloss_prefers_frontmatter_description() {
        let content = "---\ndescription: from frontmatter\n---\n# Heading\n\nBody sentence.\n";
        assert_eq!(
            title_and_gloss(content, "file"),
            ("Heading".to_string(), "from frontmatter".to_string())
        );
    }

    #[test]
    fn title_and_gloss_falls_back_to_heading_and_first_sentence() {
        let content = "# The Title\n\nFirst sentence here. Second one.\n";
        assert_eq!(
            title_and_gloss(content, "file"),
            ("The Title".to_string(), "First sentence here".to_string())
        );
    }
}
