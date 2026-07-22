//! Shared helpers for the CLI integration test binaries (`cli.rs`, `mv.rs`).

use std::fs;
use std::path::Path;
use std::process::Command;

use insta_cmd::get_cargo_bin;

/// A `Command` for the built `aigarden` binary, rooted in `dir`.
pub(crate) fn aigarden(dir: &Path) -> Command {
    let mut cmd = Command::new(get_cargo_bin("aigarden"));
    cmd.current_dir(dir);
    cmd
}

/// Write `content` to `dir/name`, creating parent dirs.
pub(crate) fn write(dir: &Path, name: &str, content: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}
