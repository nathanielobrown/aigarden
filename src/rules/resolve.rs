//! Filesystem resolution shared by the reference-integrity rules: the validation
//! half that [`crate::references`] deliberately leaves out.
//!
//! Two concerns live here — does a target path exist, and does its case match the
//! filesystem exactly. Case matching is the subtle one: macOS is
//! case-insensitive, so a wrong-cased link opens locally but 404s on a
//! case-sensitive CI. We compare each path component against the real name
//! `read_dir` reports, which is the committed case for a tracked file.

use std::path::{Component, Path, PathBuf};

/// A target worth checking on disk: a local relative path, not an external URL,
/// absolute path, `mailto:`, protocol-relative link, or pure anchor.
#[must_use]
pub(crate) fn is_checkable_local(path: &str) -> bool {
    !(path.is_empty()
        || path.starts_with('#')
        || path.starts_with('/')
        || path.starts_with("//")
        || path.contains("://")
        || path.starts_with("mailto:")
        || path.starts_with("tel:"))
}

/// Resolve `target` relative to the directory of the referring file, folding
/// away `.`/`..` lexically (never via `canonicalize`, which resolves symlinks and
/// fails on a not-yet-existent path).
#[must_use]
pub(crate) fn resolve_from_file(file_abs: &Path, target: &str) -> PathBuf {
    let base = file_abs.parent().unwrap_or(file_abs);
    normalize_lexical(&base.join(target))
}

/// Resolve `target` as repo-root-relative (for code doc-refs and the bare-path
/// root fallback), rooted at the scan directory.
#[must_use]
pub(crate) fn resolve_from_root(root: &Path, target: &str) -> PathBuf {
    normalize_lexical(&root.join(target))
}

/// The existing on-disk path for `candidate`, trying a `.md` extension for an
/// extensionless wiki-style target. `None` when nothing exists.
#[must_use]
pub(crate) fn resolve_existing(candidate: &Path) -> Option<PathBuf> {
    if candidate.exists() {
        return Some(candidate.to_path_buf());
    }
    if candidate.extension().is_none() {
        let with_md = candidate.with_extension("md");
        if with_md.exists() {
            return Some(with_md);
        }
    }
    None
}

/// Whether every component of `path` matches the real filesystem case. Assumes
/// `path` exists (call after [`resolve_existing`]). A component that `read_dir`
/// reports under a different case — the macOS case-folding trap — returns false.
#[must_use]
pub(crate) fn case_exact(path: &Path) -> bool {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                let parent: &Path = if current.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    &current
                };
                let matches = std::fs::read_dir(parent)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|entry| entry.file_name() == name);
                if !matches {
                    return false;
                }
                current.push(name);
            }
            // Roots/prefixes above the repo already carry their real case.
            other => current.push(other.as_os_str()),
        }
    }
    true
}

/// Fold `.`/`..` segments lexically, keeping the path string-resolvable without
/// touching the filesystem.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_external_and_anchor_targets() {
        assert!(!is_checkable_local("https://example.com"));
        assert!(!is_checkable_local("#section"));
        assert!(!is_checkable_local("/abs/path"));
        assert!(!is_checkable_local("mailto:a@b.com"));
        assert!(is_checkable_local("docs/guide.md"));
        assert!(is_checkable_local("../sibling.md"));
    }

    #[test]
    fn normalizes_parent_segments() {
        let resolved = resolve_from_file(Path::new("/repo/docs/a.md"), "../src/b.rs");
        assert_eq!(resolved, PathBuf::from("/repo/src/b.rs"));
    }
}
