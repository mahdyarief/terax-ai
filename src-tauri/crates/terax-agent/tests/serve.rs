//! The agent's loop driven in-process through pipes, as the app will drive
//! it through an SSH session.

use std::collections::HashMap;
use std::io::{self, PipeReader, PipeWriter};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use terax_agent::serve::{serve, Options, PROGRAM};
use terax_remote_protocol::frame::{read_frame, write_frame, MAX_FRAME};
use terax_remote_protocol::handshake::{Hello, CAP_WORKSPACE, PROTOCOL_VERSION};
use terax_remote_protocol::message::{
    decode, encode, Call, CallId, Envelope, Reply, WireError, WireExecSpec,
};
use terax_workspace_core::env::WorkspaceEnv;
use terax_workspace_core::exec::capture::Limits;
use terax_workspace_core::grep::{GrepPattern, GrepRequest};
use terax_workspace_core::host::HostWorkspace;
use terax_workspace_core::workspace::{ReadRequest, Workspace};

struct Agent {
    to_agent: Option<PipeWriter>,
    from_agent: PipeReader,
    done: Option<JoinHandle<io::Result<()>>>,
}

impl Agent {
    fn start() -> Self {
        Self::start_with(None)
    }

    fn start_with(home: Option<String>) -> Self {
        let (agent_in, to_agent) = io::pipe().unwrap();
        let (from_agent, agent_out) = io::pipe().unwrap();
        let done = thread::spawn(move || {
            let ws = Arc::new(HostWorkspace::new(WorkspaceEnv::Local));
            serve(
                agent_in,
                agent_out,
                ws,
                Options {
                    version: "9.9.9-test".into(),
                    workers: 4,
                    home,
                },
            )
        });
        Self {
            to_agent: Some(to_agent),
            from_agent,
            done: Some(done),
        }
    }

    /// Started and past the handshake.
    fn ready() -> Self {
        Self::ready_with(None)
    }

    fn ready_with(home: Option<String>) -> Self {
        let mut agent = Self::start_with(home);
        let Envelope::Hello(hello) = agent.recv().unwrap() else {
            panic!("the agent speaks first");
        };
        assert_eq!(hello.program, PROGRAM);
        agent.send(&Envelope::Hello(client_hello()));
        agent
    }

    fn send(&mut self, envelope: &Envelope) {
        self.send_raw(&encode(envelope).unwrap());
    }

    fn send_raw(&mut self, body: &[u8]) {
        write_frame(self.to_agent.as_mut().unwrap(), body, MAX_FRAME).unwrap();
    }

    fn recv(&mut self) -> Option<Envelope> {
        read_frame(&mut self.from_agent, MAX_FRAME)
            .unwrap()
            .map(|body| decode(&body).unwrap())
    }

    fn call(&mut self, id: CallId, call: Call) -> Result<Reply, WireError> {
        self.send(&Envelope::Request { id, call });
        match self.recv() {
            Some(Envelope::Response { id: got, result }) => {
                assert_eq!(got, id);
                result
            }
            other => panic!("expected a response, got {other:?}"),
        }
    }

    /// Closes the agent's input and returns how `serve` ended.
    fn finish(mut self) -> io::Result<()> {
        drop(self.to_agent.take());
        let outcome = self.done.take().unwrap().join().unwrap();
        assert!(self.recv().is_none(), "output ends with the agent");
        outcome
    }
}

fn client_hello() -> Hello {
    Hello::local("terax", "9.9.9-test", &[CAP_WORKSPACE])
}

struct Tree {
    _dir: tempfile::TempDir,
    base: String,
}

impl Tree {
    fn root(&self) -> String {
        format!("{}/root", self.base)
    }
}

fn tree() -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let ws = HostWorkspace::new(WorkspaceEnv::Local);
    let base = ws.canonicalize(&dir.path().to_string_lossy()).unwrap();
    std::fs::create_dir_all(format!("{base}/root/src")).unwrap();
    std::fs::create_dir_all(format!("{base}/outside")).unwrap();
    std::fs::write(format!("{base}/root/src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(format!("{base}/outside/secret.txt"), "secret").unwrap();
    Tree { _dir: dir, base }
}

fn set_roots(agent: &mut Agent, tree: &Tree) {
    assert_eq!(
        agent.call(
            1,
            Call::SetRoots {
                roots: vec![tree.root()]
            }
        ),
        Ok(Reply::Unit)
    );
}

fn refused(result: Result<Reply, WireError>) -> bool {
    matches!(result, Err(WireError::Protocol(_)))
}

#[test]
fn the_agent_says_hello_first_and_offers_the_workspace() {
    let mut agent = Agent::start();
    let Some(Envelope::Hello(hello)) = agent.recv() else {
        panic!("a Hello");
    };
    assert_eq!(hello.protocol, PROTOCOL_VERSION);
    assert_eq!(hello.version, "9.9.9-test");
    assert_eq!(hello.os, std::env::consts::OS);
    assert!(hello.capabilities.iter().any(|cap| cap == CAP_WORKSPACE));
    agent.send(&Envelope::Hello(client_hello()));
    agent.finish().unwrap();
}

#[test]
fn a_client_speaking_another_protocol_is_turned_away() {
    let mut agent = Agent::start();
    agent.recv().unwrap();
    let mut other = client_hello();
    other.protocol = PROTOCOL_VERSION + 1;
    agent.send(&Envelope::Hello(other));
    let error = agent.finish().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("protocol"), "{error}");
}

#[test]
fn a_request_before_the_hello_is_turned_away() {
    let mut agent = Agent::start();
    agent.recv().unwrap();
    agent.send(&Envelope::Ping { nonce: 1 });
    assert!(agent.finish().is_err());
}

#[test]
fn nothing_is_served_before_the_roots_are_set() {
    let tree = tree();
    let mut agent = Agent::ready();
    let path = format!("{}/src/main.rs", tree.root());
    assert!(refused(agent.call(1, Call::Metadata { path })));
    agent.finish().unwrap();
}

#[test]
fn requests_inside_the_roots_are_served_and_outside_refused() {
    let tree = tree();
    let mut agent = Agent::ready();
    set_roots(&mut agent, &tree);
    let main = format!("{}/src/main.rs", tree.root());
    let Ok(Reply::Read(read)) = agent.call(
        2,
        Call::Read {
            path: main.clone(),
            request: ReadRequest {
                offset: 0,
                limit: 1024,
            },
        },
    ) else {
        panic!("a read");
    };
    assert_eq!(read.bytes, b"fn main() {}\n");

    let secret = format!("{}/outside/secret.txt", tree.base);
    assert!(refused(agent.call(
        3,
        Call::Metadata {
            path: secret.clone()
        }
    )));
    assert!(refused(agent.call(
        4,
        Call::CopyFile {
            from: secret,
            to: format!("{}/copy.txt", tree.root())
        }
    )));
    assert!(refused(agent.call(
        5,
        Call::WriteAtomic {
            path: format!("{}/outside/new.txt", tree.base),
            bytes: b"x".to_vec()
        }
    )));
    assert!(!std::path::Path::new(&format!("{}/outside/new.txt", tree.base)).exists());
    assert!(refused(agent.call(
        6,
        Call::Rename {
            from: main,
            to: format!("{}/outside/main.rs", tree.base)
        }
    )));
    assert!(refused(agent.call(
        7,
        Call::Metadata {
            path: format!("{}/../outside/secret.txt", tree.root())
        }
    )));
    agent.finish().unwrap();
}

#[test]
fn io_errors_arrive_with_their_kind() {
    let tree = tree();
    let mut agent = Agent::ready();
    set_roots(&mut agent, &tree);
    let missing = format!("{}/missing.txt", tree.root());
    let Err(error) = agent.call(2, Call::Metadata { path: missing }) else {
        panic!("an error");
    };
    assert_eq!(error.into_io().kind(), io::ErrorKind::NotFound);
    let Err(error) = agent.call(
        3,
        Call::SetRoots {
            roots: vec![format!("{}/nope", tree.base)],
        },
    ) else {
        panic!("roots that do not exist are refused");
    };
    assert_eq!(error.into_io().kind(), io::ErrorKind::NotFound);
    agent.finish().unwrap();
}

#[test]
fn a_link_out_of_the_roots_is_not_followed_but_can_be_removed() {
    let tree = tree();
    let link = format!("{}/escape", tree.root());
    let target = format!("{}/outside", tree.base);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_dir(&target, &link) {
        eprintln!("skipped: no symlink privilege ({error})");
        return;
    }
    let mut agent = Agent::ready();
    set_roots(&mut agent, &tree);
    assert!(refused(agent.call(2, Call::ReadDir { path: link.clone() })));
    assert!(refused(agent.call(
        3,
        Call::Read {
            path: format!("{link}/secret.txt"),
            request: ReadRequest {
                offset: 0,
                limit: 64
            },
        }
    )));
    assert!(matches!(
        agent.call(4, Call::SymlinkMetadata { path: link.clone() }),
        Ok(Reply::Meta(_))
    ));
    #[cfg(unix)]
    let remove = Call::RemoveFile { path: link.clone() };
    #[cfg(windows)]
    let remove = Call::RemoveDirAll { path: link.clone() };
    assert_eq!(agent.call(5, remove), Ok(Reply::Unit));
    assert!(
        std::path::Path::new(&format!("{target}/secret.txt")).exists(),
        "target untouched"
    );
    agent.finish().unwrap();
}

#[test]
fn many_requests_in_flight_are_all_answered_by_id() {
    let tree = tree();
    let mut agent = Agent::ready();
    set_roots(&mut agent, &tree);
    let path = format!("{}/src/main.rs", tree.root());
    for id in 100..164 {
        agent.send(&Envelope::Request {
            id,
            call: Call::Metadata { path: path.clone() },
        });
    }
    let mut seen = HashMap::new();
    for _ in 0..64 {
        let Some(Envelope::Response { id, result }) = agent.recv() else {
            panic!("a response");
        };
        assert!(matches!(result, Ok(Reply::Meta(_))));
        *seen.entry(id).or_insert(0) += 1;
    }
    assert_eq!(seen.len(), 64);
    assert!(seen.values().all(|count| *count == 1));
    agent.finish().unwrap();
}

#[test]
fn ping_is_answered_and_a_cancelled_search_is_still_answered() {
    let tree = tree();
    let mut agent = Agent::ready();
    set_roots(&mut agent, &tree);
    agent.send(&Envelope::Ping { nonce: 42 });
    assert_eq!(agent.recv(), Some(Envelope::Pong { nonce: 42 }));

    agent.send(&Envelope::Request {
        id: 7,
        call: Call::Grep {
            request: GrepRequest {
                root: tree.root(),
                pattern: GrepPattern::Literal {
                    text: "main".into(),
                },
                globs: Vec::new(),
                max_results: None,
            },
        },
    });
    agent.send(&Envelope::Cancel { id: 7 });
    agent.send(&Envelope::Cancel { id: 999 }); // unknown: ignored
    let Some(Envelope::Response {
        id: 7,
        result: Ok(Reply::Grep(_)),
    }) = agent.recv()
    else {
        panic!("the search is answered, cancelled or not");
    };
    agent.finish().unwrap();
}

#[test]
fn exec_starts_in_the_first_root_unless_told_and_never_outside() {
    let tree = tree();
    let mut agent = Agent::ready();
    set_roots(&mut agent, &tree);
    let spec = |cwd: Option<String>| WireExecSpec {
        program: "git".into(),
        args: vec!["rev-parse".into(), "--show-prefix".into()],
        cwd,
        env: Vec::new(),
        env_remove: Vec::new(),
        stdin: None,
        timeout_ms: 30_000,
        limits: Limits {
            stdout: 4096,
            stderr: 4096,
            stop_on_stdout_cap: false,
        },
    };
    assert!(refused(agent.call(
        2,
        Call::Exec {
            spec: spec(Some(format!("{}/outside", tree.base)))
        }
    )));
    // Not a repository: git says so, which shows where it started.
    let Ok(Reply::Exec(ran)) = agent.call(3, Call::Exec { spec: spec(None) }) else {
        panic!("git runs");
    };
    assert_ne!(ran.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&ran.stderr).contains("not a git repository"));
    agent.finish().unwrap();
}

#[test]
fn a_message_that_cannot_be_decoded_ends_the_session() {
    let mut agent = Agent::ready();
    agent.send_raw(&[0xc1]);
    let error = agent.finish().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn the_end_of_input_ends_the_session_cleanly() {
    Agent::ready().finish().unwrap();
    // Even before the handshake.
    let mut agent = Agent::start();
    agent.recv().unwrap();
    agent.finish().unwrap();
}

#[test]
fn the_workspace_reached_through_the_agent_is_the_same_one() {
    let tree = tree();
    let mut agent = Agent::ready();
    set_roots(&mut agent, &tree);
    let path = format!("{}/new.txt", tree.root());
    assert!(matches!(
        agent.call(
            2,
            Call::WriteAtomic {
                path: path.clone(),
                bytes: b"hello".to_vec()
            }
        ),
        Ok(Reply::Written(_))
    ));
    let direct = HostWorkspace::new(WorkspaceEnv::Local);
    assert_eq!(
        direct
            .read(
                &path,
                ReadRequest {
                    offset: 0,
                    limit: 64
                }
            )
            .unwrap()
            .bytes,
        b"hello"
    );
    agent.finish().unwrap();
}

#[test]
fn writing_through_a_file_link_that_points_out_is_refused() {
    let tree = tree();
    let link = format!("{}/notes.txt", tree.root());
    let target = format!("{}/outside/secret.txt", tree.base);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(&target, &link) {
        eprintln!("skipped: no symlink privilege ({error})");
        return;
    }
    let mut agent = Agent::ready();
    set_roots(&mut agent, &tree);
    assert!(refused(agent.call(
        2,
        Call::WriteAtomic {
            path: link.clone(),
            bytes: b"overwritten".to_vec()
        }
    )));
    assert!(refused(agent.call(
        3,
        Call::CopyFile {
            from: format!("{}/src/main.rs", tree.root()),
            to: link
        }
    )));
    assert_eq!(std::fs::read(&target).unwrap(), b"secret");
    agent.finish().unwrap();
}

#[test]
fn an_entry_named_dot_dot_cannot_reach_the_roots_parent() {
    let tree = tree();
    let mut agent = Agent::ready();
    set_roots(&mut agent, &tree);
    for path in [
        format!("{}/..", tree.root()),
        format!("{}/src/../..", tree.root()),
    ] {
        assert!(
            refused(agent.call(2, Call::RemoveDirAll { path: path.clone() })),
            "{path}"
        );
    }
    assert!(std::path::Path::new(&format!("{}/outside/secret.txt", tree.base)).exists());
    assert!(std::path::Path::new(&tree.root()).exists());
    agent.finish().unwrap();
}

#[test]
fn home_is_the_users_canonical_home_or_a_clear_error() {
    let tree = tree();
    let mut agent = Agent::ready_with(Some(format!("{}/root/src/..", tree.base)));
    assert_eq!(agent.call(1, Call::Home), Ok(Reply::Path(tree.root())));
    agent.finish().unwrap();

    let mut agent = Agent::ready();
    match agent.call(1, Call::Home) {
        Err(WireError::Io { message, .. }) => assert!(message.contains("HOME"), "{message}"),
        other => panic!("{other:?}"),
    }
    agent.finish().unwrap();
}

#[test]
fn browsing_lists_only_directories_and_works_outside_the_roots() {
    let tree = tree();
    std::fs::write(format!("{}/top.txt", tree.base), "x").unwrap();
    let mut agent = Agent::ready();
    // No roots at all: every file call is refused...
    assert!(matches!(
        agent.call(
            1,
            Call::ReadDir {
                path: tree.base.clone()
            }
        ),
        Err(WireError::Protocol(_))
    ));
    // ...but a folder picker can browse, and sees directories only.
    let Ok(Reply::Browse(browse)) = agent.call(
        2,
        Call::BrowseDirs {
            path: format!("{}/root/..", tree.base),
        },
    ) else {
        panic!("a listing");
    };
    assert_eq!(browse.path, tree.base, "canonical");
    assert_eq!(browse.dirs, ["outside", "root"], "no top.txt");
    let Ok(Reply::Browse(src)) = agent.call(
        3,
        Call::BrowseDirs {
            path: format!("{}/src", tree.root()),
        },
    ) else {
        panic!("a listing");
    };
    assert!(src.dirs.is_empty(), "main.rs is not a directory");
    // A file is not browsable, so nothing about it comes back.
    match agent.call(
        4,
        Call::BrowseDirs {
            path: format!("{}/outside/secret.txt", tree.base),
        },
    ) {
        Err(WireError::Io { message, .. }) => assert!(!message.contains(
            "secret
"
        )),
        other => panic!("{other:?}"),
    }
    agent.finish().unwrap();
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "the agent runs on Linux servers and refuses macOS"
)]
fn watched_folders_and_files_report_changes_until_the_set_is_emptied() {
    let tree = tree();
    let mut agent = Agent::ready();
    set_roots(&mut agent, &tree);
    let src = format!("{}/src", tree.root());
    let main = format!("{src}/main.rs");
    let watch = Call::SetWatch {
        // Outside the roots and missing: left out, not an error.
        dirs: vec![
            src.clone(),
            format!("{}/outside", tree.base),
            format!("{}/nope", tree.root()),
        ],
        files: vec![main.clone()],
    };
    assert_eq!(agent.call(2, watch), Ok(Reply::Unit));

    std::fs::write(format!("{src}/new.rs"), "").unwrap();
    let Some(Envelope::Changed { paths }) = agent.recv() else {
        panic!("a new file is reported");
    };
    assert_eq!(paths, std::slice::from_ref(&src));

    std::fs::write(&main, "fn main() { changed() }\n").unwrap();
    let Some(Envelope::Changed { paths }) = agent.recv() else {
        panic!("an edit of a watched file is reported");
    };
    assert_eq!(paths, std::slice::from_ref(&main));

    let stop = Call::SetWatch {
        dirs: Vec::new(),
        files: Vec::new(),
    };
    assert_eq!(agent.call(3, stop), Ok(Reply::Unit));
    std::fs::write(format!("{src}/later.rs"), "").unwrap();
    std::fs::write(&main, "fn main() {}\n").unwrap();
    // Past the debounce window: nothing came before the pong.
    thread::sleep(std::time::Duration::from_millis(1500));
    agent.send(&Envelope::Ping { nonce: 9 });
    assert_eq!(agent.recv(), Some(Envelope::Pong { nonce: 9 }));
    agent.finish().unwrap();
}
