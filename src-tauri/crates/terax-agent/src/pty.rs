//! Terminals on the server (`Call::PtyOpen` and the `Pty*` envelopes).
//!
//! Each terminal is a portable-pty pair with three threads of its own, never
//! a request worker: one writes keystrokes, one waits for the process, one
//! reads output. The reader sends at most `PTY_CHUNK` bytes a frame and stops
//! reading while `PTY_WINDOW` bytes are uncredited, so a flood waits in the
//! terminal (as it would locally) instead of filling the stream that file
//! replies share. `PtyExit` follows the last `PtyData`: the waiter takes the
//! reader's last word (or gives up on it after `DRAIN`, when a background
//! process still holds the terminal) before it reports the end.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use terax_remote_protocol::message::{Envelope, PtyId, PtySpec, PTY_CHUNK, PTY_WINDOW};

/// Sends an envelope to the client; it may be gone, which the read loop
/// notices on its own.
pub type Emit = Arc<dyn Fn(Envelope) + Send + Sync>;

/// How long an ended process's output is still read before `PtyExit`.
const DRAIN: Duration = Duration::from_secs(2);

/// Bytes sent and not yet credited, and whether the terminal has ended.
#[derive(Default)]
struct Window {
    state: Mutex<(u64, bool)>,
    changed: Condvar,
}

impl Window {
    /// Waits for room in the window and returns how much there is (a
    /// read may take no more, so the window is never exceeded); `None` once
    /// the terminal has ended. A credit that lands later only adds room.
    fn wait_for_room(&self) -> Option<u64> {
        let mut state = self.state.lock().unwrap();
        while state.0 >= PTY_WINDOW && !state.1 {
            state = self.changed.wait(state).unwrap();
        }
        (!state.1).then(|| PTY_WINDOW - state.0)
    }

    fn credit(&self, bytes: u64) {
        let mut state = self.state.lock().unwrap();
        state.0 = state.0.saturating_sub(bytes);
        self.changed.notify_all();
    }

    fn end(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}

struct Pty {
    input: mpsc::Sender<Vec<u8>>,
    /// Dropped when the process ends (on Windows that is what ends the
    /// reader); `None` from then on.
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    window: Window,
    /// Held while a `PtyData` or the `PtyExit` is sent; true once the exit
    /// has been, so nothing follows it.
    ended: Mutex<bool>,
    /// Why it ended, when it did not end by itself.
    reason: Mutex<Option<String>>,
}

impl Pty {
    fn kill(&self, reason: &str) {
        self.reason
            .lock()
            .unwrap()
            .get_or_insert_with(|| reason.to_string());
        let _ = self.killer.lock().unwrap().kill();
    }
}

/// A session's terminals by id.
#[derive(Clone, Default)]
pub struct Ptys(Arc<Mutex<HashMap<PtyId, Arc<Pty>>>>);

impl Ptys {
    /// Starts a terminal in `cwd` (already checked against the roots);
    /// returns the program it runs.
    pub fn open(
        &self,
        id: PtyId,
        spec: &PtySpec,
        cwd: &str,
        home: Option<&str>,
        emit: Emit,
    ) -> io::Result<String> {
        if self.0.lock().unwrap().contains_key(&id) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("terminal {id} is already open"),
            ));
        }
        let size = PtySize {
            rows: spec.rows.max(1),
            cols: spec.cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = native_pty_system().openpty(size).map_err(other)?;
        let (cmd, program) = command(spec, cwd, home)?;
        let mut child = pair.slave.spawn_command(cmd).map_err(other)?;
        drop(pair.slave);
        let mut killer = child.clone_killer();
        let pipes = pair
            .master
            .try_clone_reader()
            .and_then(|reader| Ok((reader, pair.master.take_writer()?)));
        let (reader, mut writer) = match pipes {
            Ok(pipes) => pipes,
            Err(error) => {
                let _ = killer.kill();
                return Err(other(error));
            }
        };

        let (input, keys) = mpsc::channel::<Vec<u8>>();
        let pty = Arc::new(Pty {
            input,
            master: Mutex::new(Some(pair.master)),
            killer: Mutex::new(killer),
            window: Window::default(),
            ended: Mutex::new(false),
            reason: Mutex::new(None),
        });
        self.0.lock().unwrap().insert(id, pty.clone());

        thread::spawn(move || {
            for bytes in keys {
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    return;
                }
            }
        });

        let (drained, reader_done) = mpsc::channel::<()>();
        {
            let (pty, emit) = (pty.clone(), emit.clone());
            thread::spawn(move || {
                pump(reader, &pty.window, &pty.ended, id, &emit);
                let _ = drained.send(());
            });
        }

        let ptys = self.clone();
        thread::spawn(move || {
            let code = child.wait().ok().map(|status| status.exit_code());
            // Closing the terminal ends the reader on Windows; on Unix the
            // reader holds its own descriptor and reads what is left.
            pty.master.lock().unwrap().take();
            let _ = reader_done.recv_timeout(DRAIN);
            ptys.0.lock().unwrap().remove(&id);
            let mut ended = pty.ended.lock().unwrap();
            *ended = true;
            pty.window.end();
            let reason = pty.reason.lock().unwrap().take();
            emit(Envelope::PtyExit { id, code, reason });
        });
        Ok(program)
    }

    /// Keystrokes for a terminal; an unknown id has just ended.
    pub fn write(&self, id: PtyId, bytes: Vec<u8>) {
        if let Some(pty) = self.get(id) {
            let _ = pty.input.send(bytes);
        }
    }

    pub fn resize(&self, id: PtyId, cols: u16, rows: u16) {
        let Some(pty) = self.get(id) else { return };
        let master = pty.master.lock().unwrap();
        if let Some(master) = master.as_ref() {
            let _ = master.resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    pub fn credit(&self, id: PtyId, bytes: u64) {
        if let Some(pty) = self.get(id) {
            pty.window.credit(bytes);
        }
    }

    /// Kills the terminal's process; its `PtyExit` follows.
    pub fn close(&self, id: PtyId) {
        if let Some(pty) = self.get(id) {
            pty.kill("the terminal was closed");
        }
    }

    /// Kills every terminal (the session is over).
    pub fn close_all(&self, reason: &str) {
        let open: Vec<Arc<Pty>> = self.0.lock().unwrap().values().cloned().collect();
        for pty in open {
            pty.window.end();
            pty.kill(reason);
        }
    }

    fn get(&self, id: PtyId) -> Option<Arc<Pty>> {
        self.0.lock().unwrap().get(&id).cloned()
    }
}

/// Reads output in chunks while the window has room, until the terminal
/// ends or the reader does.
fn pump(mut reader: impl Read, window: &Window, ended: &Mutex<bool>, id: PtyId, emit: &Emit) {
    let mut buffer = vec![0u8; PTY_CHUNK];
    while let Some(room) = window.wait_for_room() {
        let room = (room as usize).min(PTY_CHUNK);
        let read = match reader.read(&mut buffer[..room]) {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        let ended = ended.lock().unwrap();
        if *ended {
            return;
        }
        window.state.lock().unwrap().0 += read as u64;
        emit(Envelope::PtyData {
            id,
            bytes: buffer[..read].to_vec(),
        });
        drop(ended);
    }
}

/// A bare program name found on `PATH`. ConPTY's `CreateProcessW` does not
/// search it (the agent runs on Windows only in tests); on Unix the command
/// builder does.
#[cfg(windows)]
fn on_path(program: &str) -> std::ffi::OsString {
    if program.contains(['/', '\\']) {
        return program.into();
    }
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(format!("{program}.exe")))
                .find(|candidate| candidate.is_file())
        })
        .map_or_else(|| program.into(), Into::into)
}

#[cfg(not(windows))]
fn on_path(program: &str) -> std::ffi::OsString {
    program.into()
}

/// The command a terminal runs, with the environment the app gives its own
/// terminals (no control endpoint: the app is not on this machine).
fn command(spec: &PtySpec, cwd: &str, home: Option<&str>) -> io::Result<(CommandBuilder, String)> {
    let (mut cmd, program) = match &spec.command {
        Some(argv) => {
            let program = argv.first().cloned().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "an empty terminal command")
            })?;
            let mut cmd = CommandBuilder::new(on_path(&program));
            cmd.args(&argv[1..]);
            (cmd, program)
        }
        None => login_shell(home),
    };
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERAX_TERMINAL", "1");
    if spec.blocks {
        cmd.env("TERAX_BLOCKS", "1");
    }
    let is_utf8 = |value: &str| {
        let value = value.to_ascii_uppercase();
        value.contains("UTF-8") || value.contains("UTF8")
    };
    let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .any(|key| std::env::var(key).is_ok_and(|v| is_utf8(&v)));
    if !utf8 {
        cmd.env("LANG", "C.UTF-8");
    }
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    Ok((cmd, program))
}

#[cfg(unix)]
fn login_shell(home: Option<&str>) -> (CommandBuilder, String) {
    let path = std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    let mut cmd = CommandBuilder::new(&path);
    match home {
        Some(home) => {
            let zdotdir = std::env::var("ZDOTDIR").ok();
            let setup = integration::setup(&path, home, zdotdir.as_deref());
            match integration::write(&setup) {
                Ok(()) => setup.apply(&mut cmd),
                Err(error) => log::warn!("shell integration for {path} disabled: {error}"),
            }
        }
        None => log::warn!("shell integration for {path} disabled: no home directory"),
    }
    (cmd, path)
}

/// The agent runs on Windows only in tests; its shell has no integration.
#[cfg(windows)]
fn login_shell(_home: Option<&str>) -> (CommandBuilder, String) {
    let path = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
    (CommandBuilder::new(&path), path)
}

fn other(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    /// Output without end, as fast as it is read.
    struct Flood;

    impl Read for Flood {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            buffer.fill(b'x');
            Ok(buffer.len())
        }
    }

    fn settle(sent: &AtomicU64) -> u64 {
        let mut last = u64::MAX;
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            thread::sleep(Duration::from_millis(50));
            let now = sent.load(Ordering::SeqCst);
            if now == last {
                return now;
            }
            last = now;
        }
        panic!("the reader never stopped");
    }

    /// Output in reads of an odd size, as a Unix terminal gives it (never a
    /// multiple of the chunk).
    struct OddFlood;

    impl Read for OddFlood {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let n = buffer.len().min(4095);
            buffer[..n].fill(b'y');
            Ok(n)
        }
    }

    #[test]
    fn reads_of_any_size_never_go_past_the_window() {
        let window = Arc::new(Window::default());
        let ended = Arc::new(Mutex::new(false));
        let sent = Arc::new(AtomicU64::new(0));
        let emit: Emit = {
            let sent = sent.clone();
            Arc::new(move |envelope| {
                let Envelope::PtyData { bytes, .. } = envelope else {
                    panic!("only data");
                };
                sent.fetch_add(bytes.len() as u64, Ordering::SeqCst);
            })
        };
        let pumping = {
            let (window, ended) = (window.clone(), ended.clone());
            thread::spawn(move || pump(OddFlood, &window, &ended, 7, &emit))
        };
        assert_eq!(
            settle(&sent),
            PTY_WINDOW,
            "exactly the window, never past it"
        );
        window.credit(10_000);
        assert_eq!(settle(&sent), PTY_WINDOW + 10_000);
        window.end();
        pumping.join().unwrap();
    }

    #[test]
    fn a_flood_stops_at_the_window_and_moves_on_with_credit() {
        let window = Arc::new(Window::default());
        let ended = Arc::new(Mutex::new(false));
        let sent = Arc::new(AtomicU64::new(0));
        let frames = Arc::new(AtomicU64::new(0));
        let emit: Emit = {
            let (sent, frames) = (sent.clone(), frames.clone());
            Arc::new(move |envelope| {
                let Envelope::PtyData { id: 7, bytes } = envelope else {
                    panic!("only data, for its own id");
                };
                assert!(bytes.len() <= PTY_CHUNK);
                sent.fetch_add(bytes.len() as u64, Ordering::SeqCst);
                frames.fetch_add(1, Ordering::SeqCst);
            })
        };
        let pumping = {
            let (window, ended) = (window.clone(), ended.clone());
            thread::spawn(move || pump(Flood, &window, &ended, 7, &emit))
        };
        assert_eq!(settle(&sent), PTY_WINDOW, "stops with a full window");
        assert_eq!(frames.load(Ordering::SeqCst), PTY_WINDOW / PTY_CHUNK as u64);
        window.credit(3 * PTY_CHUNK as u64);
        assert_eq!(settle(&sent), PTY_WINDOW + 3 * PTY_CHUNK as u64);
        // Ending wakes a reader waiting for room, and it sends no more.
        window.end();
        pumping.join().unwrap();
        assert_eq!(
            sent.load(Ordering::SeqCst),
            PTY_WINDOW + 3 * PTY_CHUNK as u64
        );
    }

    #[test]
    fn nothing_is_sent_after_the_exit() {
        let window = Window::default();
        let ended = Mutex::new(true);
        let emit: Emit = Arc::new(|envelope| panic!("sent {envelope:?} after the exit"));
        pump(Flood, &window, &ended, 1, &emit);
    }
}

/// The app's shell integration (`src/modules/pty/scripts`), set up on the
/// server as the app sets it up locally on Unix (`shell_init.rs`).
#[cfg_attr(windows, allow(dead_code))]
pub(crate) mod integration {
    use std::io;
    use std::path::Path;

    use portable_pty::CommandBuilder;

    const ZSHENV: &str = include_str!("../../../src/modules/pty/scripts/zshenv.zsh");
    const ZPROFILE: &str = include_str!("../../../src/modules/pty/scripts/zprofile.zsh");
    const ZLOGIN: &str = include_str!("../../../src/modules/pty/scripts/zlogin.zsh");
    const ZSHRC: &str = include_str!("../../../src/modules/pty/scripts/zshrc.zsh");
    const BASHRC: &str = include_str!("../../../src/modules/pty/scripts/bashrc.bash");
    const FISH_INIT: &str = include_str!("../../../src/modules/pty/scripts/init.fish");
    const FISH_REINSTALL_PROMPT: &str =
        "functions -q __terax_install_prompt; and __terax_install_prompt";

    /// What a shell is started with, and the files it reads.
    #[derive(Debug, Default, PartialEq, Eq)]
    pub struct Setup {
        pub args: Vec<String>,
        pub env: Vec<(String, String)>,
        pub env_remove: Vec<String>,
        pub files: Vec<(String, &'static str)>,
    }

    impl Setup {
        pub fn apply(&self, cmd: &mut CommandBuilder) {
            cmd.args(&self.args);
            for (key, value) in &self.env {
                cmd.env(key, value);
            }
            for key in &self.env_remove {
                cmd.env_remove(key);
            }
        }
    }

    /// The setup for the shell at `shell_path`, with files under `home`.
    /// `zdotdir` is the agent's own `ZDOTDIR` (the user's, on a server).
    pub fn setup(shell_path: &str, home: &str, zdotdir: Option<&str>) -> Setup {
        let home = home.trim_end_matches('/');
        let root = format!("{home}/.cache/terax/shell-integration");
        let mut setup = Setup::default();
        match shell_path.rsplit('/').next().unwrap_or("") {
            "zsh" => {
                let dir = format!("{root}/zsh");
                setup.files = vec![
                    (format!("{dir}/.zshenv"), ZSHENV),
                    (format!("{dir}/.zprofile"), ZPROFILE),
                    (format!("{dir}/.zlogin"), ZLOGIN),
                    (format!("{dir}/.zshrc"), ZSHRC),
                ];
                match zdotdir.filter(|user| !user.is_empty() && *user != dir) {
                    Some(user) => setup
                        .env
                        .push(("TERAX_USER_ZDOTDIR".into(), user.to_string())),
                    None => setup.env_remove.push("TERAX_USER_ZDOTDIR".into()),
                }
                setup.env.push(("ZDOTDIR".into(), dir));
                setup.args.push("-l".into());
            }
            "bash" => {
                let rc = format!("{root}/bash/bashrc");
                setup.files = vec![(rc.clone(), BASHRC)];
                // bash ignores --rcfile under -l; the rcfile sources the
                // login files itself.
                setup.args = vec!["--rcfile".into(), rc, "-i".into()];
            }
            "fish" => {
                setup.files = vec![(format!("{home}/.config/fish/conf.d/terax.fish"), FISH_INIT)];
                // fish 4 writes its own prompt marks; ours would double them.
                setup
                    .env
                    .push(("fish_features".into(), "no-mark-prompt".into()));
                setup.args = vec!["-i".into(), "-C".into(), FISH_REINSTALL_PROMPT.into()];
            }
            _ => {}
        }
        setup
    }

    /// Writes the setup's files, each replaced atomically (a shell starting
    /// at the same time must never read half of one). Line ends are made
    /// Unix ones: an agent built from a Windows checkout carries the scripts
    /// with CRLF, which bash on the server refuses.
    pub fn write(setup: &Setup) -> io::Result<()> {
        for (path, content) in &setup.files {
            let content = content.replace("\r\n", "\n");
            let path = Path::new(path);
            if std::fs::read_to_string(path).is_ok_and(|existing| existing == content) {
                continue;
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let tmp = path.with_extension("__terax_tmp__");
            std::fs::write(&tmp, &content)?;
            if let Err(error) = std::fs::rename(&tmp, path) {
                let _ = std::fs::remove_file(&tmp);
                return Err(error);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn each_shell_gets_the_apps_integration_under_the_servers_home() {
            let zsh = setup("/usr/bin/zsh", "/home/me/", None);
            let dir = "/home/me/.cache/terax/shell-integration/zsh";
            assert_eq!(zsh.args, ["-l"]);
            assert_eq!(zsh.env, [("ZDOTDIR".to_string(), dir.to_string())]);
            assert_eq!(zsh.env_remove, ["TERAX_USER_ZDOTDIR"]);
            assert_eq!(zsh.files.len(), 4);
            assert!(zsh.files.iter().all(|(path, _)| path.starts_with(dir)));

            let own = setup("/bin/zsh", "/home/me", Some("/home/me/.zsh"));
            assert!(own
                .env
                .contains(&("TERAX_USER_ZDOTDIR".into(), "/home/me/.zsh".into())));
            // A zsh inside a Terax terminal: ZDOTDIR is ours, not the user's.
            let nested = setup("/bin/zsh", "/home/me", Some(dir));
            assert_eq!(nested.env_remove, ["TERAX_USER_ZDOTDIR"]);

            let bash = setup("/bin/bash", "/home/me", None);
            assert_eq!(
                bash.args,
                [
                    "--rcfile",
                    "/home/me/.cache/terax/shell-integration/bash/bashrc",
                    "-i"
                ]
            );
            assert_eq!(bash.files[0].1, BASHRC);

            let fish = setup("/usr/local/bin/fish", "/home/me", None);
            assert_eq!(fish.files[0].0, "/home/me/.config/fish/conf.d/terax.fish");
            assert_eq!(fish.args[..2], ["-i", "-C"]);

            assert_eq!(setup("/bin/sh", "/home/me", None), Setup::default());
        }

        #[test]
        fn files_are_written_once_and_left_when_unchanged() {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().to_string_lossy().replace('\\', "/");
            let bash = setup("/bin/bash", &home, None);
            write(&bash).unwrap();
            let rc = &bash.files[0].0;
            let written = std::fs::read_to_string(rc).unwrap();
            assert_eq!(written, BASHRC.replace("\r\n", "\n"));
            assert!(
                !written.contains('\r'),
                "Unix line ends whatever the checkout"
            );
            let stamp = std::fs::metadata(rc).unwrap().modified().unwrap();
            write(&bash).unwrap();
            assert_eq!(std::fs::metadata(rc).unwrap().modified().unwrap(), stamp);
        }
    }
}
