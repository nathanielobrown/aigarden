//! Thin CLI shell: parse args, delegate to the library, propagate the exit code.

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = ailint::Cli::parse();
    ailint::run(&cli)
}
