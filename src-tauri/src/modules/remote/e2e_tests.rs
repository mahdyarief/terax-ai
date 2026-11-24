//! End to end against a real server, not run by default: the app's registry
//! and `SshConnector` with the user's `ssh`, the agent bundled from
//! `target/`, and `terax-cli` as askpass answering a stand-in endpoint.
//!
//! ```text
//! cargo build -p terax-cli
//! pnpm build:agent        # or the x86_64 musl agent in target/
//! TERAX_AGENT_SSH_HOST=myserver \
//! cargo test -p terax --lib remote::e2e_tests -- --ignored --nocapture
//! ```
//!
//! Every login succeeds (the user's own key; the new host key is accepted),
//! so the server sees no failed attempt: fail2ban banned this machine once.
//! The test installs under `~/.terax-server/0.0.0-e2e-<pid>-*` only and
//! removes those directories at the end.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use terax_control_protocol::askpass::{classify, AskpassKind, AskpassParams, CANCELLED};
use terax_control_protocol::{ControlRequest, ControlResponse};
use terax_workspace_core::env::SshHost;
use terax_workspace_core::workspace::{EntryKind, Workspace};

use super::connector::SshConnector;
use super::machine::State;
use super::registry::Registry;
use crate::modules::control::ControlState;
use crate::modules::remote_agent::{dev_root, AgentBundle};

type Asked = Arc<Mutex<Vec<(AskpassKind, String)>>>;

/// Answers ssh's questions: a new host key is accepted, anything else is
/// cancelled (this server takes the user's key; no password is expected).
fn endpoint() -> (String, Asked) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let asked: Asked = Arc::default();
    let record = Arc::clone(&asked);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut line = String::new();
            if BufReader::new(&mut stream).read_line(&mut line).is_err() {
                continue;
            }
            let Ok(request) = serde_json::from_str::<ControlRequest>(&line) else {
                continue;
            };
            let params: AskpassParams = serde_json::from_value(request.params).unwrap();
            let kind = classify(&params.prompt, params.hint.as_deref());
            record.lock().unwrap().push((kind, params.prompt.clone()));
            let response = match kind {
                AskpassKind::YesNo { host_key: true } => {
                    ControlResponse::success(request.id, serde_json::json!({ "answer": "yes" }))
                }
                _ => ControlResponse::failure(request.id, CANCELLED, "not expected"),
            };
            let mut bytes = serde_json::to_vec(&response).unwrap();
            bytes.push(b'\n');
            let _ = stream.write_all(&bytes);
        }
    });
    (address, asked)
}

fn cli() -> PathBuf {
    let name = if cfg!(windows) {
        "terax-cli.exe"
    } else {
        "terax-cli"
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("debug")
        .join(name);
    assert!(path.exists(), "run `cargo build -p terax-cli` first");
    path
}

fn server(host: &str, script: &str) -> String {
    let output = Command::new("ssh")
        .args(["-o", "BatchMode=yes", host, script])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Removes this run's directories from the server (`~` expanded there).
struct Cleanup {
    host: String,
    paths: Vec<String>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        server(&self.host, &format!("rm -rf {}", self.paths.join(" ")));
    }
}

fn registry(version: &str, control: &ControlState, known_hosts: &Path) -> Registry {
    let connector = SshConnector {
        ssh: "ssh".into(),
        version: version.into(),
        agents: AgentBundle::new(None, dev_root().map(PathBuf::from)),
        control: control.clone(),
        ssh_options: vec![
            format!("UserKnownHostsFile={}", known_hosts.display()),
            "StrictHostKeyChecking=ask".into(),
        ],
    };
    Registry::new(Arc::new(connector), |_| {})
}

fn wait(registry: &Registry, host: &SshHost, what: &str, done: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let state = registry.state(host);
        if done(&state) {
            return;
        }
        assert!(
            !matches!(state, State::Failed { .. }) && Instant::now() < deadline,
            "waiting for {what}; now {state:?}\n{}",
            registry.log(host).join("\n")
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn installs(registry: &Registry, host: &SshHost) -> usize {
    registry
        .log(host)
        .iter()
        .filter(|line| line.as_str() == "the agent was installed")
        .count()
}

#[test]
#[ignore = "needs TERAX_AGENT_SSH_HOST"]
fn connect_install_reconnect_and_upgrade_on_a_real_server() {
    let name = std::env::var("TERAX_AGENT_SSH_HOST").expect("set TERAX_AGENT_SSH_HOST");
    let host = SshHost::try_from(name.as_str()).unwrap();
    let prefix = format!("0.0.0-e2e-{}", std::process::id());
    let _cleanup = Cleanup {
        host: name.clone(),
        paths: vec![
            format!("~/.terax-server/{prefix}-a"),
            format!("~/.terax-server/{prefix}-b"),
        ],
    };
    let (address, asked) = endpoint();
    let control = ControlState::for_askpass_tests(&address, cli());
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");

    // A first connect: the new host key is asked through terax-cli, the
    // agent is installed.
    let a = registry(&format!("{prefix}-a"), &control, &known_hosts);
    let started = Instant::now();
    a.connect(&host);
    wait(&a, &host, "ready", |s| s == &State::Ready);
    println!("first connect with install: {:?}", started.elapsed());
    {
        let asked = asked.lock().unwrap();
        assert!(
            matches!(
                asked.first(),
                Some((AskpassKind::YesNo { host_key: true }, _))
            ),
            "the new host key is asked: {asked:?}"
        );
        assert_eq!(asked.len(), 1, "{asked:?}");
    }
    assert!(known_hosts.exists(), "the accepted key was recorded");
    assert_eq!(installs(&a, &host), 1);
    let workspace = a.workspace(&host).unwrap();
    assert!(workspace.metadata("/tmp").is_err(), "no roots yet");
    // The folder picker's calls work before any root.
    let home = workspace.home().unwrap();
    assert!(home.starts_with('/'), "{home}");
    let browse = workspace.browse_dirs(&home).unwrap();
    assert_eq!(browse.path, home);
    assert!(
        browse.dirs.iter().any(|d| d == ".terax-server"),
        "{:?}",
        browse.dirs
    );
    println!("home {home}: {} directories", browse.dirs.len());
    a.set_roots(&host, vec!["/tmp".into()]).unwrap();
    assert_eq!(workspace.metadata("/tmp").unwrap().kind, EntryKind::Dir);

    // Disconnect and connect again: the installed agent is reused, and the
    // now known host key is not asked again.
    a.disconnect(&host);
    wait(&a, &host, "idle", |s| s == &State::Idle);
    let started = Instant::now();
    a.connect(&host);
    wait(&a, &host, "ready again", |s| s == &State::Ready);
    println!("reconnect reusing the agent: {:?}", started.elapsed());
    assert_eq!(installs(&a, &host), 1, "reused");
    assert_eq!(asked.lock().unwrap().len(), 1, "no new question");

    // The session dies under it (the agent is killed on the server): the
    // registry notices and reconnects on its own, without prompting, and
    // puts the roots back.
    server(
        &name,
        &format!("pkill -f '[.]terax-server/{prefix}-a/terax-agent'"),
    );
    wait(&a, &host, "reconnecting", |s| {
        matches!(s, State::Reconnecting { .. })
    });
    let started = Instant::now();
    wait(&a, &host, "ready after the drop", |s| s == &State::Ready);
    println!("automatic reconnect after a drop: {:?}", started.elapsed());
    let reconnecting = a
        .log(&host)
        .into_iter()
        .find(|line| line.starts_with("state: reconnecting"))
        .unwrap();
    assert!(
        !reconnecting.contains("Permanently added"),
        "an earlier session's warning is not the reason: {reconnecting}"
    );
    let workspace = a.workspace(&host).unwrap();
    assert_eq!(
        workspace.metadata("/tmp").unwrap().kind,
        EntryKind::Dir,
        "roots put back"
    );
    assert_eq!(installs(&a, &host), 1);
    assert_eq!(asked.lock().unwrap().len(), 1);

    // Another version installs beside the first, which stays.
    let b = registry(&format!("{prefix}-b"), &control, &known_hosts);
    b.connect(&host);
    wait(&b, &host, "ready on b", |s| s == &State::Ready);
    assert_eq!(installs(&b, &host), 1);
    let listed = server(&name, "ls ~/.terax-server");
    assert!(listed.contains(&format!("{prefix}-a")), "{listed}");
    assert!(listed.contains(&format!("{prefix}-b")), "{listed}");
    assert!(
        b.log(&host)
            .iter()
            .any(|line| line.starts_with("ssh: OpenSSH")),
        "the ssh version is logged"
    );

    a.disconnect(&host);
    b.disconnect(&host);
    wait(&a, &host, "a idle", |s| s == &State::Idle);
    wait(&b, &host, "b idle", |s| s == &State::Idle);
    println!("log of a:\n{}", a.log(&host).join("\n"));
}

/// What the renderer asks the asset scheme for.
fn asset_request(path: &str, token: &str) -> tauri::http::Request<Vec<u8>> {
    let encoded =
        percent_encoding::utf8_percent_encode(path, percent_encoding::NON_ALPHANUMERIC).to_string();
    tauri::http::Request::builder()
        .uri(format!("https://asset.localhost/{encoded}?token={token}"))
        .body(Vec::new())
        .unwrap()
}

/// A folder on the server through the app's own command functions, on the
/// app-wide registry (what `backend_for` asks). Run it alone: the registry
/// is installed once per process.
#[test]
#[ignore = "needs TERAX_AGENT_SSH_HOST"]
fn a_folder_on_a_real_server_through_the_app_commands() {
    use std::sync::mpsc;

    use terax_workspace_core::env::WorkspaceEnv;

    use super::watch::{push, resume, Deliver, RemoteWatches, WatchSet};
    use crate::modules::fs::ai;
    use crate::modules::fs::assets::{respond, AssetGrants};
    use crate::modules::fs::file::{read_file_sync, ReadResult};
    use crate::modules::fs::grep::run_grep;
    use crate::modules::fs::index::FileIndexState;
    use crate::modules::fs::mutate::{create_dir, create_file, remove_path, rename};
    use crate::modules::fs::search::{run_search, SearchOptions};
    use crate::modules::fs::tree::read_dir_entries;
    use crate::modules::git::operations;
    use crate::modules::pty::remote::probe::Probe;
    use crate::modules::pty::remote::CONNECTION_ENDED_EXIT;
    use crate::modules::shell::run_command_in;
    use crate::modules::shell::session::ShellSession;
    use crate::modules::workspace::{backend_for, WorkspaceRegistry};

    let name = std::env::var("TERAX_AGENT_SSH_HOST").expect("set TERAX_AGENT_SSH_HOST");
    let host = SshHost::try_from(name.as_str()).unwrap();
    let pid = std::process::id();
    let prefix = format!("0.0.0-e2e-{pid}");
    let folder = format!("/tmp/terax-e2e-{pid}");
    let _cleanup = Cleanup {
        host: name.clone(),
        paths: vec![format!("~/.terax-server/{prefix}-c"), folder.clone()],
    };
    server(
        &name,
        &format!(
            "mkdir -p {folder}/src {folder}/img && printf 'fn main() {{}}\\n' > {folder}/src/main.rs \
             && printf '# hi\\n' > {folder}/README.md && printf 'PNG_BYTES' > {folder}/img/logo.png"
        ),
    );
    let (address, _asked) = endpoint();
    let control = ControlState::for_askpass_tests(&address, cli());
    let dir = tempfile::tempdir().unwrap();
    assert!(
        super::registry::global().is_none(),
        "run this test alone: the app-wide registry is already installed"
    );
    super::registry::install(registry(
        &format!("{prefix}-c"),
        &control,
        &dir.path().join("known_hosts"),
    ));
    let reg = super::registry::global().unwrap();
    reg.connect(&host);
    wait(reg, &host, "ready", |s| s == &State::Ready);

    // Open the folder: authorized, then listed by the explorer.
    let env = WorkspaceEnv::Ssh { host: host.clone() };
    let root = WorkspaceRegistry::default()
        .authorize_in(&env, &folder)
        .unwrap();
    let names = |dir: &str| -> Vec<String> {
        let mut names: Vec<_> = read_dir_entries(dir.into(), false, Some(true), Some(env.clone()))
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        names.sort();
        names
    };
    assert_eq!(names(&root), ["README.md", "img", "src"]);

    // Edit and save; a change made on the server moves the stamp the
    // editor's conflict check compares.
    let ws = backend_for(&env);
    let main = format!("{root}/src/main.rs");
    let ReadResult::Text { content, .. } = read_file_sync(&*ws, &main, false).unwrap() else {
        panic!("text");
    };
    assert_eq!(content, "fn main() {}\n");
    let saved = ws.write_atomic(&main, b"fn main() { run() }\n").unwrap();
    assert_eq!(
        ws.metadata(&main).unwrap().mtime_ms,
        saved.mtime_ms,
        "the save's stamp"
    );
    thread::sleep(Duration::from_millis(1100));
    server(&name, &format!("printf 'changed elsewhere\\n' > {main}"));
    assert_ne!(ws.metadata(&main).unwrap().mtime_ms, saved.mtime_ms);

    // Create, rename and delete.
    create_dir(&*ws, &format!("{root}/docs")).unwrap();
    create_file(&*ws, &format!("{root}/docs/new.txt")).unwrap();
    rename(
        &*ws,
        &format!("{root}/docs/new.txt"),
        &format!("{root}/docs/renamed.txt"),
    )
    .unwrap();
    assert_eq!(names(&format!("{root}/docs")), ["renamed.txt"]);
    remove_path(&*ws, &format!("{root}/docs")).unwrap();
    assert_eq!(names(&root), ["README.md", "img", "src"]);

    // Find a file, and text in files.
    let index = FileIndexState::default();
    let found = run_search(
        &index,
        root.clone(),
        "main".into(),
        None,
        Some(env.clone()),
        None,
        SearchOptions {
            files_only: true,
            with_indices: false,
        },
        &|| false,
    )
    .unwrap();
    assert_eq!(
        found
            .hits
            .iter()
            .map(|h| h.path.clone())
            .collect::<Vec<_>>(),
        std::slice::from_ref(&main)
    );
    let grep = run_grep(
        "elsewhere".into(),
        root.clone(),
        None,
        None,
        None,
        Some(env.clone()),
    )
    .unwrap();
    assert_eq!(
        grep.hits.iter().map(|h| h.rel.clone()).collect::<Vec<_>>(),
        ["src/main.rs"]
    );

    // A change made over a separate ssh shows up through the watch.
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let deliver: Deliver = Arc::new(move |_: &SshHost, paths: Vec<String>| {
        let _ = tx.lock().unwrap().send(paths);
    });
    let remote = reg.workspace(&host).unwrap();
    resume(&remote, &host, WatchSet::default(), deliver.clone()).unwrap();
    let watches = RemoteWatches::default();
    let src = format!("{root}/src");
    let set = watches.add(&host, std::slice::from_ref(&src), &[]).unwrap();
    push(Some(remote), set).unwrap();
    let started = Instant::now();
    server(&name, &format!("touch {src}/added.rs"));
    let changed = rx.recv_timeout(Duration::from_secs(20)).expect("a change");
    println!("watch delivered after {:?}", started.elapsed());
    assert_eq!(changed, std::slice::from_ref(&src));

    // An image preview.
    let grants = AssetGrants::default();
    let logo = format!("{root}/img/logo.png");
    let token = grants.grant("main".into(), &logo, env.clone(), 0).unwrap();
    let served = respond(&grants, "main", &asset_request(&logo, &token)).unwrap();
    assert_eq!(served.body(), b"PNG_BYTES");

    // Source control: git on the server, the explorer's dimming from it.
    server(
        &name,
        &format!(
            "cd {folder} && git init -q -b main && git config user.name e2e \
             && git config user.email e2e@example.invalid && git add . \
             && git commit -q -m initial"
        ),
    );
    let git_registry = WorkspaceRegistry::default();
    git_registry.authorize_in(&env, &folder).unwrap();
    ws.write_atomic(&format!("{root}/notes.md"), b"notes\n")
        .unwrap();
    ws.write_atomic(&format!("{root}/.gitignore"), b"*.log\n")
        .unwrap();
    ws.write_atomic(&format!("{root}/debug.log"), b"").unwrap();
    let status = operations::status(&git_registry, &root, &env).unwrap();
    let mut changed: Vec<_> = status
        .changed_files
        .iter()
        .map(|f| (f.path.clone(), f.untracked))
        .collect();
    changed.sort();
    assert_eq!(
        changed,
        [
            (".gitignore".to_string(), true),
            ("notes.md".to_string(), true)
        ]
    );
    operations::stage(
        &git_registry,
        &root,
        &["notes.md".into(), ".gitignore".into()],
        &env,
    )
    .unwrap();
    operations::commit(&git_registry, &root, "from the app", &env).unwrap();
    let log = operations::log(&git_registry, &root, 5, None, &env).unwrap();
    assert_eq!(
        log.iter().map(|e| e.subject.as_str()).collect::<Vec<_>>(),
        ["from the app", "initial"]
    );
    let dimmed: Vec<String> = read_dir_entries(root.clone(), true, Some(true), Some(env.clone()))
        .unwrap()
        .into_iter()
        .filter(|e| e.gitignored)
        .map(|e| e.name)
        .collect();
    assert_eq!(dimmed, ["debug.log"]);

    // The AI's tools: search and read on the server, its secrets left out,
    // and commands run there.
    ws.write_atomic(&format!("{root}/server.key"), b"notes\n")
        .unwrap();
    let glob = ai::run_glob(&git_registry, "**/*.md".into(), root.clone(), None, &env).unwrap();
    assert!(
        glob.hits.iter().any(|hit| hit.rel == "notes.md"),
        "{:?}",
        glob.hits.len()
    );
    let grep = ai::run_grep(
        &git_registry,
        "^notes$".into(),
        root.clone(),
        None,
        None,
        None,
        &env,
    )
    .unwrap();
    let rels: Vec<_> = grep.hits.iter().map(|hit| hit.rel.as_str()).collect();
    assert_eq!(rels, ["notes.md"]);
    let read = ai::read_file_in(&git_registry, &env, &format!("{root}/notes.md"), 1024).unwrap();
    assert!(matches!(read, ReadResult::Text { ref content, .. } if content == "notes\n"));
    assert!(ai::read_file_in(&git_registry, &env, &format!("{root}/server.key"), 1024).is_err());
    let out = run_command_in(
        &git_registry,
        "uname -s; pwd".into(),
        Some(root.clone()),
        env.clone(),
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(out.stdout, format!("Linux\n{root}\n"));
    ws.create_dir_all(&format!("{root}/sub")).unwrap();
    let session = ShellSession::new(root.clone(), env.clone());
    let moved = session
        .run("cd sub".into(), None, None, Duration::from_secs(30))
        .unwrap();
    assert_eq!(moved.cwd_after, format!("{root}/sub"));
    let here = session
        .run("pwd".into(), None, None, Duration::from_secs(30))
        .unwrap();
    assert_eq!(here.stdout.trim(), format!("{root}/sub"));

    // A terminal on the server, through the app's session: its folder, a
    // resize, and a flood while the explorer keeps answering.
    let mut terminal = Probe::open(&host, Some(root.clone()), 1);
    terminal.write("pwd\r");
    // The prompt names the folder too, but only `pwd` ends a line with it.
    terminal.read_until(|out| out.contains(&format!("{root}\r\n")));
    terminal.resize(133, 41);
    terminal.write("tput cols; tput lines\r");
    terminal.read_until(|out| out.contains("133\r\n41"));
    let started = Instant::now();
    terminal.write("seq 1 300000; echo flood-$((6*7))\r");
    let listing = thread::scope(|scope| {
        let listing = scope.spawn(|| {
            thread::sleep(Duration::from_millis(300));
            let started = Instant::now();
            names(&root);
            started.elapsed()
        });
        terminal.read_until(|out| out.contains("flood-42"));
        listing.join().unwrap()
    });
    println!(
        "terminal flood in {:?}; a listing beside it took {listing:?}",
        started.elapsed()
    );
    assert!(listing < Duration::from_secs(3), "{listing:?}");

    // The connection drops: the terminal ends and says so (its pane offers a
    // new one), the connection comes back on its own, the explorer lists
    // again, and the watch is resumed with one catch-up.
    server(
        &name,
        &format!("pkill -f '[.]terax-server/{prefix}-c/terax-agent'"),
    );
    wait(reg, &host, "reconnecting", |s| {
        matches!(s, State::Reconnecting { .. })
    });
    assert_eq!(terminal.wait_exit(), CONNECTION_ENDED_EXIT);
    assert!(terminal
        .output()
        .contains(&format!("[the connection to {name} ended]")));
    let started = Instant::now();
    wait(reg, &host, "ready after the drop", |s| s == &State::Ready);
    println!("reconnect after a drop: {:?}", started.elapsed());
    // Retry: a new terminal on the reconnected server.
    let mut again = Probe::open(&host, Some(root.clone()), 2);
    again.write("echo again-$((1+1)); exit 3\r");
    assert_eq!(again.wait_exit(), 3);
    assert!(again.output().contains("again-2"));
    assert_eq!(names(&src), ["added.rs", "main.rs"]);
    index.forget_host(&host);
    let remote = reg.workspace(&host).unwrap();
    resume(&remote, &host, watches.set(&host), deliver).unwrap();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        std::slice::from_ref(&src),
        "one catch-up"
    );

    reg.disconnect(&host);
    wait(reg, &host, "idle", |s| s == &State::Idle);
    println!("log:\n{}", reg.log(&host).join("\n"));
}
