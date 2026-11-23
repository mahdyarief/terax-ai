//! `terax-agent` serves on stdin and stdout until stdin ends; stdout carries
//! only protocol frames, diagnostics go to stderr. `--version` prints the
//! version, which the app compares with its own before it starts a session.

use std::process::ExitCode;
use std::sync::Arc;

use terax_agent::serve::{serve, Options, PROGRAM};
use terax_workspace_core::env::WorkspaceEnv;
use terax_workspace_core::host::HostWorkspace;

fn main() -> ExitCode {
    let options = Options::default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] => {}
        ["--version"] => {
            println!("{PROGRAM} {}", options.version);
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("usage: {PROGRAM} [--version]");
            return ExitCode::from(2);
        }
    }
    let ws = Arc::new(HostWorkspace::new(WorkspaceEnv::Local));
    match serve(std::io::stdin().lock(), std::io::stdout(), ws, options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{PROGRAM}: {error}");
            ExitCode::FAILURE
        }
    }
}
