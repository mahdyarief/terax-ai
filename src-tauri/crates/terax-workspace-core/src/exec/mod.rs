//! Running a program in a workspace: on this machine, inside a WSL
//! distribution, and later on a remote server. Output is bounded and the run
//! has a deadline. Callers (git, the shell tool) decide arguments and
//! environment; this module only decides where and how the process runs.

pub mod capture;

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::env::WorkspaceEnv;
use capture::{Captured, Limits};

/// One bounded run of a program.
pub struct ExecSpec {
    pub program: String,
    pub args: Vec<OsString>,
    /// Working directory as a workspace path; `None` keeps the default.
    pub cwd: Option<String>,
    /// Variables to set, applied after `env_remove`.
    pub env: Vec<(OsString, OsString)>,
    /// Variables to remove from the inherited environment.
    pub env_remove: Vec<OsString>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    pub limits: Limits,
}

/// Keeps a console window from flashing up for a child on Windows.
#[cfg(windows)]
pub fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
#[inline]
pub fn hide_console(_cmd: &mut Command) {}

/// The command that runs `program` in the workspace: directly for a local
/// workspace, through `wsl.exe --cd ... --exec` for a WSL one.
pub fn build_command(
    _workspace: &WorkspaceEnv,
    program: &str,
    cwd: Option<&str>,
    args: &[OsString],
) -> io::Result<Command> {
    #[cfg(windows)]
    if let WorkspaceEnv::Wsl { distro } = _workspace {
        crate::env::validate_wsl_distro_name(distro)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let mut cmd = Command::new("wsl.exe");
        cmd.arg("-d").arg(distro);
        if let Some(cwd) = cwd.filter(|s| !s.is_empty()) {
            cmd.arg("--cd").arg(cwd);
        }
        cmd.arg("--exec").arg(program);
        cmd.args(args);
        return Ok(cmd);
    }

    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(dir) = cwd.filter(|s| !s.is_empty()) {
        cmd.current_dir(Path::new(dir));
    }
    Ok(cmd)
}

/// Runs `spec` in the workspace on this machine.
pub fn run_on_host(workspace: &WorkspaceEnv, spec: ExecSpec) -> io::Result<Captured> {
    let mut cmd = build_command(workspace, &spec.program, spec.cwd.as_deref(), &spec.args)?;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    for name in &spec.env_remove {
        cmd.env_remove(name);
    }
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    cmd.stdin(if spec.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    hide_console(&mut cmd);
    capture::run_with_input(&mut cmd, spec.timeout, spec.limits, spec.stdin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn builds_a_wsl_command_with_cd_and_exec() {
        let cmd = build_command(
            &WorkspaceEnv::Wsl {
                distro: "Ubuntu".into(),
            },
            "git",
            Some("/home/vinicios/Nova pasta/repo"),
            &[OsString::from("status"), OsString::from("--short")],
        )
        .expect("valid WSL distro");
        let program = cmd.get_program().to_string_lossy().into_owned();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(program, "wsl.exe");
        assert_eq!(
            args,
            vec![
                "-d",
                "Ubuntu",
                "--cd",
                "/home/vinicios/Nova pasta/repo",
                "--exec",
                "git",
                "status",
                "--short",
            ]
        );
    }

    #[test]
    fn a_local_command_runs_in_its_working_directory() {
        let cmd = build_command(&WorkspaceEnv::Local, "git", Some("/repo"), &["status".into()]).unwrap();
        assert_eq!(cmd.get_program(), "git");
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/repo")));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, ["status"]);
    }
}
