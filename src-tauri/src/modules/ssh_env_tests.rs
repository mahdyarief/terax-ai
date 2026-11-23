//! An SSH workspace never starts anything on this machine: every gate that
//! leads to a local process refuses it, with or without a working directory.

use terax_workspace_core::env::SshHost;

use crate::modules::workspace::{
    authorize_spawn_cwd, authorize_user_spawn_cwd, backend_for, WorkspaceEnv, WorkspaceRegistry,
};

fn ssh() -> WorkspaceEnv {
    WorkspaceEnv::Ssh {
        host: SshHost::try_from("dypod").unwrap(),
    }
}

#[test]
fn spawn_gates_refuse_a_remote_workspace_even_without_a_cwd() {
    let registry = WorkspaceRegistry::default();
    let here = std::env::temp_dir().to_string_lossy().into_owned();
    for cwd in [None, Some(here.as_str())] {
        assert!(
            authorize_spawn_cwd(&registry, cwd, &ssh()).is_err(),
            "{cwd:?}"
        );
        assert!(
            authorize_user_spawn_cwd(&registry, cwd, &ssh()).is_err(),
            "{cwd:?}"
        );
    }
    assert!(authorize_spawn_cwd(&registry, None, &WorkspaceEnv::Local)
        .unwrap()
        .is_none());
}

#[test]
fn no_terminal_or_shell_command_is_built_for_a_remote_workspace() {
    let pty = crate::modules::pty::shell_init::build_command(None, ssh(), false, None, None);
    assert!(pty.is_err());
    assert!(crate::modules::shell::build_oneshot_command("echo hi", &ssh(), None).is_err());
    assert!(
        crate::modules::shell::build_oneshot_command("echo hi", &ssh(), Some("/home/me")).is_err()
    );
}

#[test]
fn the_backend_for_an_unconnected_host_is_not_connected() {
    let error = backend_for(&ssh()).metadata("/home/me").unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotConnected);
    assert!(error.to_string().contains("dypod"), "{error}");
}
