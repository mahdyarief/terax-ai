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
    keep_stdio_from_children();
    terax_agent::log::install();
    let ws = Arc::new(HostWorkspace::new(WorkspaceEnv::Local));
    match serve(std::io::stdin().lock(), std::io::stdout(), ws, options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{PROGRAM}: {error}");
            ExitCode::FAILURE
        }
    }
}

/// On Windows a child started with `bInheritHandles` gets every inheritable
/// handle, including the agent's own stdin and stdout when they were made
/// inheritable for it. A command run by `exec` would then hold the protocol
/// stream open after the agent dies, and the app would not see the agent go
/// until that command ended. Unix closes those descriptors on its own: the
/// child's 0, 1 and 2 are replaced and the rest are close-on-exec.
#[cfg(windows)]
fn keep_stdio_from_children() {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE] {
        // SAFETY: a handle from GetStdHandle, possibly null or invalid, which
        // SetHandleInformation rejects without harm.
        unsafe {
            SetHandleInformation(GetStdHandle(which), HANDLE_FLAG_INHERIT, 0);
        }
    }
}

#[cfg(not(windows))]
fn keep_stdio_from_children() {}
