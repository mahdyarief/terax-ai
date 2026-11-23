//! An SSH workspace's paths are the server's. Nothing on this machine may
//! read, write or run anything for them; only `backend_for`'s connection can.

use std::io::ErrorKind;
use std::time::Duration;

use terax_workspace_core::confined::{Confine, ConfinedError, ConfinedRead};
use terax_workspace_core::env::{resolve_path, SshHost, WorkspaceEnv, UNREACHABLE};
use terax_workspace_core::exec::capture::Limits;
use terax_workspace_core::exec::{build_command, ExecSpec};
use terax_workspace_core::grep::{GlobRequest, GrepPattern, GrepRequest};
use terax_workspace_core::host::HostWorkspace;
use terax_workspace_core::unavailable::Unavailable;
use terax_workspace_core::workspace::{ReadRequest, Workspace};

fn ssh() -> WorkspaceEnv {
    WorkspaceEnv::Ssh {
        host: SshHost::try_from("dypod").unwrap(),
    }
}

#[test]
fn hosts_that_ssh_could_misread_are_refused() {
    for good in [
        "dypod",
        "me@example.com",
        "me@10.0.0.2",
        "box:2222",
        "[::1]:22",
        "a.b-c_d",
    ] {
        assert_eq!(SshHost::try_from(good).unwrap().as_str(), good);
    }
    let long = "a".repeat(256);
    for bad in [
        "",
        "-oProxyCommand=calc",
        "host name",
        "host\tname",
        "host\nname",
        "host\0",
        "a;b",
        "a|b",
        "a&b",
        "$(id)",
        "`id`",
        "a'b",
        "a\"b",
        long.as_str(),
    ] {
        assert!(SshHost::try_from(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn an_ssh_env_deserializes_and_a_bad_host_does_not() {
    let env: WorkspaceEnv = serde_json::from_str(r#"{"kind":"ssh","host":"dypod"}"#).unwrap();
    assert_eq!(env, ssh());
    assert!(env.is_ssh() && !env.is_local() && !env.is_wsl());
    assert!(
        serde_json::from_str::<WorkspaceEnv>(r#"{"kind":"ssh","host":"-oProxyCommand=x"}"#)
            .is_err()
    );
    assert!(serde_json::from_str::<WorkspaceEnv>(r#"{"kind":"ssh"}"#).is_err());
}

#[test]
fn a_remote_path_maps_to_nothing_on_this_machine() {
    let here = std::env::temp_dir().to_string_lossy().into_owned();
    let mapped = resolve_path(&here, &ssh());
    assert_eq!(mapped.to_str(), Some(UNREACHABLE));
    // Every way of touching it fails before it reaches the disk, joined or not.
    for path in [mapped.clone(), mapped.join("child")] {
        assert_eq!(
            std::fs::metadata(&path).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            std::fs::read_dir(&path).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            std::fs::create_dir_all(&path).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            std::fs::write(&path, b"x").unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            std::fs::canonicalize(&path).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert!(!path.exists());
        let ran = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .current_dir(&path)
            .output();
        assert!(ran.is_err(), "a process cannot start in it");
    }
}

#[test]
fn no_process_is_built_for_a_remote_workspace() {
    let error = build_command(&ssh(), "git", None, &[]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert!(build_command(&ssh(), "git", Some("/home/me"), &[]).is_err());
}

#[test]
fn the_host_backend_reaches_nothing_for_a_remote_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let here = dir.path().to_string_lossy().replace('\\', "/");
    std::fs::write(dir.path().join("a.txt"), "local").unwrap();
    let ws = HostWorkspace::new(ssh());
    let file = format!("{here}/a.txt");

    assert!(ws.metadata(&file).is_err());
    assert!(ws.symlink_metadata(&file).is_err());
    assert!(ws.canonicalize(&here).is_err());
    assert!(ws.read_dir(&here).is_err());
    assert!(ws
        .read(
            &file,
            ReadRequest {
                offset: 0,
                limit: 64
            }
        )
        .is_err());
    let confined = ConfinedRead {
        roots: vec![Confine {
            path: here.clone(),
            subtree: true,
        }],
        secret_policy: false,
        offset: 0,
        limit: 64,
    };
    assert!(matches!(
        ws.read_confined(&file, &confined),
        Err(ConfinedError::Io(_))
    ));
    assert!(ws.write_atomic(&format!("{here}/b.txt"), b"x").is_err());
    assert!(ws.create_file(&format!("{here}/c.txt")).is_err());
    assert!(ws.create_dir_all(&format!("{here}/d")).is_err());
    assert!(ws.copy_file(&file, &format!("{here}/e.txt")).is_err());
    assert!(ws.rename(&file, &format!("{here}/f.txt")).is_err());
    assert!(ws.remove_file(&file).is_err());
    let grep = GrepRequest {
        root: here.clone(),
        pattern: GrepPattern::Literal {
            text: "local".into(),
        },
        globs: Vec::new(),
        max_results: None,
    };
    assert!(ws
        .grep(&grep, &|| false)
        .map_or(true, |found| found.hits.is_empty()));
    assert!(ws
        .glob(&GlobRequest {
            root: here.clone(),
            pattern: "*".into(),
            max_results: None
        })
        .map_or(true, |found| found.hits.is_empty()));
    assert!(ws
        .list_files(&here, true, None, &|| false)
        .is_none_or(|index| index.entries.iter().next().is_none()));
    assert_eq!(
        ws.exec(ExecSpec {
            program: "git".into(),
            args: vec!["status".into()],
            cwd: Some(here.clone()),
            env: Vec::new(),
            env_remove: Vec::new(),
            stdin: None,
            timeout: Duration::from_secs(5),
            limits: Limits {
                stdout: 64,
                stderr: 64,
                stop_on_stdout_cap: false
            },
        })
        .unwrap_err()
        .kind(),
        ErrorKind::Unsupported
    );

    // The local directory of the same name is exactly as it was.
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["a.txt"]);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "local"
    );
}

#[test]
fn an_unavailable_workspace_fails_everything_with_not_connected() {
    let ws = Unavailable::new(ssh(), "not connected to dypod");
    assert_eq!(ws.env(), &ssh());
    let error = ws.metadata("/home/me").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::NotConnected);
    assert_eq!(error.to_string(), "not connected to dypod");
    assert_eq!(
        ws.read_dir("/home/me").unwrap_err().kind(),
        ErrorKind::NotConnected
    );
    assert_eq!(
        ws.write_atomic("/home/me/a", b"x").unwrap_err().kind(),
        ErrorKind::NotConnected
    );
    assert!(ws.list_files("/home/me", false, None, &|| false).is_none());
    let grep = GrepRequest {
        root: "/home/me".into(),
        pattern: GrepPattern::Literal { text: "x".into() },
        globs: Vec::new(),
        max_results: None,
    };
    assert_eq!(
        ws.grep(&grep, &|| false).unwrap_err(),
        "not connected to dypod"
    );
}
