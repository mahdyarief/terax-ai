//! The agent's loop: one byte stream in, one out (an SSH session's stdio).
//!
//! The agent says `Hello` first, reads the client's and checks the protocol.
//! Then each `Request` runs on a worker, so a slow search does not hold up a
//! `stat`; responses go out as they finish, matched by id. `SetRoots` runs in
//! order on the reading thread, so every request after it sees the new roots.
//! `Cancel` raises the flag a search polls; the request is still answered.
//! Terminals (`pty.rs`) open in order too, so the keystrokes sent after a
//! `PtyOpen` find their terminal; they run on threads of their own.
//! When the input ends, in-flight work is cancelled and waited for, the
//! terminals are killed, and `serve` returns.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread;

use terax_remote_protocol::frame::{read_frame, write_frame, FrameError, MAX_FRAME};
use terax_remote_protocol::handshake::{negotiate, Hello, CAP_PTY, CAP_WORKSPACE};
use terax_remote_protocol::message::{
    decode, encode, Browse, Call, CallId, Envelope, Reply, WireError,
};
use terax_workspace_core::confined::ConfinedRead;
use terax_workspace_core::exec::ExecSpec;
use terax_workspace_core::workspace::{EntryKind, Workspace};

use crate::pty::{Emit, Ptys};
use crate::roots::{Refusal, Roots};
use crate::watch::Watches;

pub const PROGRAM: &str = "terax-agent";

pub struct Options {
    pub version: String,
    /// Requests that may run at once.
    pub workers: usize,
    /// The user's home directory, for `Home` (`$HOME` by default).
    pub home: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        let cores = thread::available_parallelism().map_or(4, |n| n.get());
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            workers: cores.clamp(2, 8),
            home: std::env::var("HOME").ok().filter(|h| !h.is_empty()),
        }
    }
}

type Output = Arc<Mutex<Box<dyn Write + Send>>>;
type InFlight = Arc<Mutex<HashMap<CallId, Arc<AtomicBool>>>>;

struct Job {
    id: CallId,
    call: Call,
    cancel: Arc<AtomicBool>,
}

/// Serves `ws` until `input` ends. `Ok` on a clean end of input; an error
/// for a protocol mismatch, a malformed frame or a message it cannot decode.
pub fn serve(
    mut input: impl Read,
    output: impl Write + Send + 'static,
    ws: Arc<dyn Workspace>,
    options: Options,
) -> io::Result<()> {
    let output: Output = Arc::new(Mutex::new(Box::new(output)));
    let ours = Hello::local(PROGRAM, &options.version, &[CAP_WORKSPACE, CAP_PTY]);
    send(&output, &Envelope::Hello(ours.clone()))?;
    match read_envelope(&mut input)? {
        Some(Envelope::Hello(theirs)) => {
            negotiate(&ours, &theirs, &[]).map_err(|e| invalid(e.to_string()))?;
        }
        Some(other) => return Err(invalid(format!("expected Hello, got {other:?}"))),
        None => return Ok(()),
    }

    let watches = {
        let output = output.clone();
        Watches::new(Arc::new(move |paths| {
            // The client may be gone; the read loop notices when input ends.
            let _ = send(&output, &Envelope::Changed { paths });
        }))
    };
    let home = Arc::new(options.home);
    let roots = Arc::new(RwLock::new(Roots::default()));
    let in_flight: InFlight = Arc::default();
    let (jobs, queue) = mpsc::channel::<Job>();
    let queue = Arc::new(Mutex::new(queue));
    let workers: Vec<_> = (0..options.workers.max(1))
        .map(|_| {
            let (queue, output, ws, roots, in_flight, home) = (
                queue.clone(),
                output.clone(),
                ws.clone(),
                roots.clone(),
                in_flight.clone(),
                home.clone(),
            );
            thread::spawn(move || loop {
                let job = match queue.lock().unwrap().recv() {
                    Ok(job) => job,
                    Err(_) => return,
                };
                let roots = roots.read().unwrap().clone();
                let result = handle(ws.as_ref(), &roots, home.as_deref(), job.call, &job.cancel);
                in_flight.lock().unwrap().remove(&job.id);
                // The client may be gone; the loop notices when input ends.
                let _ = respond(&output, job.id, result);
            })
        })
        .collect();

    let ptys = Ptys::default();
    let emit: Emit = {
        let output = output.clone();
        Arc::new(move |envelope| {
            // The client may be gone; the read loop notices when input ends.
            let _ = send(&output, &envelope);
        })
    };
    let outcome = read_loop(
        &mut input,
        &Session {
            output: &output,
            ws: ws.as_ref(),
            roots: &roots,
            watches: &watches,
            in_flight: &in_flight,
            jobs: &jobs,
            ptys: &ptys,
            home: home.as_deref(),
            emit: &emit,
        },
    );
    drop(watches);
    ptys.close_all("the session ended");

    for cancel in in_flight.lock().unwrap().values() {
        cancel.store(true, Ordering::Relaxed);
    }
    drop(jobs);
    for worker in workers {
        let _ = worker.join();
    }
    outcome
}

/// What the reading thread works with.
struct Session<'a> {
    output: &'a Output,
    ws: &'a dyn Workspace,
    roots: &'a RwLock<Roots>,
    watches: &'a Watches,
    in_flight: &'a InFlight,
    jobs: &'a mpsc::Sender<Job>,
    ptys: &'a Ptys,
    home: Option<&'a str>,
    emit: &'a Emit,
}

fn read_loop(input: &mut impl Read, session: &Session) -> io::Result<()> {
    let Session {
        output,
        ws,
        roots,
        watches,
        in_flight,
        jobs,
        ptys,
        home,
        emit,
    } = *session;
    while let Some(envelope) = read_envelope(input)? {
        match envelope {
            Envelope::Request {
                id,
                call: Call::SetRoots { roots: wanted },
            } => {
                let result = Roots::new(ws, &wanted)
                    .map(|new| {
                        *roots.write().unwrap() = new;
                        Reply::Unit
                    })
                    .map_err(|e| WireError::from_io(&e));
                respond(output, id, result)?;
            }
            // In order with `SetRoots`, and against the roots it set.
            Envelope::Request {
                id,
                call: Call::SetWatch { dirs, files },
            } => {
                let roots = roots.read().unwrap().clone();
                let result = watches
                    .set(ws, &roots, &dirs, &files)
                    .map(|()| Reply::Unit)
                    .map_err(|e| WireError::from_io(&e));
                respond(output, id, result)?;
            }
            // In order, so later keystrokes find the terminal.
            Envelope::Request {
                id,
                call: Call::PtyOpen { id: pty, spec },
            } => {
                let roots = roots.read().unwrap().clone();
                let result = pty_cwd(ws, &roots, spec.cwd.as_deref()).and_then(|cwd| {
                    ptys.open(pty, &spec, &cwd, home, emit.clone())
                        .map(|program| Reply::PtyOpened { program })
                        .map_err(|e| WireError::from_io(&e))
                });
                respond(output, id, result)?;
            }
            Envelope::Request { id, call } => {
                let cancel = Arc::new(AtomicBool::new(false));
                let duplicate = match in_flight.lock().unwrap().entry(id) {
                    Entry::Occupied(_) => true,
                    Entry::Vacant(slot) => {
                        slot.insert(cancel.clone());
                        false
                    }
                };
                if duplicate {
                    let why = format!("request {id} is already in flight");
                    respond(output, id, Err(WireError::Protocol(why)))?;
                } else {
                    let _ = jobs.send(Job { id, call, cancel });
                }
            }
            Envelope::Cancel { id } => {
                if let Some(cancel) = in_flight.lock().unwrap().get(&id) {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
            Envelope::Ping { nonce } => send(output, &Envelope::Pong { nonce })?,
            Envelope::Pong { .. } => {}
            Envelope::PtyWrite { id, bytes } => ptys.write(id, bytes),
            Envelope::PtyResize { id, cols, rows } => ptys.resize(id, cols, rows),
            Envelope::PtyClose { id } => ptys.close(id),
            Envelope::PtyCredit { id, bytes } => ptys.credit(id, bytes),
            other @ (Envelope::Hello(_)
            | Envelope::Response { .. }
            | Envelope::Changed { .. }
            | Envelope::PtyData { .. }
            | Envelope::PtyExit { .. }) => {
                return Err(invalid(format!(
                    "unexpected message from the client: {other:?}"
                )));
            }
        }
    }
    Ok(())
}

/// Runs one call after checking every path it names against the roots.
fn handle(
    ws: &dyn Workspace,
    roots: &Roots,
    home: Option<&str>,
    call: Call,
    cancel: &AtomicBool,
) -> Result<Reply, WireError> {
    let io = |e: io::Error| WireError::from_io(&e);
    let refused = |r: Refusal| WireError::Protocol(r.to_string());
    let follow = |path: &str| roots.follow(ws, path).map_err(refused);
    let entry = |path: &str| roots.entry(ws, path).map_err(refused);
    // Written through: the entry and where it leads must both be inside.
    let write = |path: &str| entry(path).and_then(|()| follow(path));

    match call {
        Call::SetRoots { .. } => Err(WireError::Protocol("SetRoots runs in order".into())),
        Call::SetWatch { .. } => Err(WireError::Protocol("SetWatch runs in order".into())),
        Call::PtyOpen { .. } => Err(WireError::Protocol("PtyOpen runs in order".into())),
        Call::Home => match home {
            Some(home) => ws.canonicalize(home).map(Reply::Path).map_err(io),
            None => Err(io(io::Error::new(
                io::ErrorKind::NotFound,
                "the server has no home directory ($HOME is not set)",
            ))),
        },
        Call::BrowseDirs { path } => browse(ws, &path).map(Reply::Browse).map_err(io),
        Call::Metadata { path } => {
            follow(&path)?;
            ws.metadata(&path).map(Reply::Meta).map_err(io)
        }
        Call::SymlinkMetadata { path } => {
            entry(&path)?;
            ws.symlink_metadata(&path).map(Reply::Meta).map_err(io)
        }
        Call::Canonicalize { path } => {
            follow(&path)?;
            ws.canonicalize(&path).map(Reply::Path).map_err(io)
        }
        Call::ReadLink { path } => {
            entry(&path)?;
            ws.read_link(&path).map(Reply::Path).map_err(io)
        }
        Call::ReadDir { path } => {
            follow(&path)?;
            ws.read_dir(&path).map(Reply::Entries).map_err(io)
        }
        Call::Read { path, request } => {
            follow(&path)?;
            ws.read(&path, request).map(Reply::Read).map_err(io)
        }
        Call::ReadConfined { path, request } => {
            follow(&path)?;
            for root in &request.roots {
                follow(&root.path)?;
            }
            read_confined(ws, &path, &request)
        }
        Call::WriteAtomic { path, bytes } => {
            write(&path)?;
            ws.write_atomic(&path, &bytes)
                .map(Reply::Written)
                .map_err(io)
        }
        Call::CreateFile { path } => {
            write(&path)?;
            ws.create_file(&path).map(|()| Reply::Unit).map_err(io)
        }
        Call::CreateDirAll { path } => {
            follow(&path)?;
            ws.create_dir_all(&path).map(|()| Reply::Unit).map_err(io)
        }
        Call::Rename { from, to } => {
            entry(&from)?;
            entry(&to)?;
            ws.rename(&from, &to).map(|()| Reply::Unit).map_err(io)
        }
        Call::ReplaceFile { from, to } => {
            entry(&from)?;
            entry(&to)?;
            ws.replace_file(&from, &to)
                .map(|()| Reply::Unit)
                .map_err(io)
        }
        Call::RemoveFile { path } => {
            entry(&path)?;
            ws.remove_file(&path).map(|()| Reply::Unit).map_err(io)
        }
        Call::RemoveDirAll { path } => {
            entry(&path)?;
            ws.remove_dir_all(&path).map(|()| Reply::Unit).map_err(io)
        }
        Call::CopyFile { from, to } => {
            follow(&from)?;
            write(&to)?;
            ws.copy_file(&from, &to).map(Reply::Copied).map_err(io)
        }
        // The walks do not follow links, so the root is the only path to check.
        Call::Grep { request } => {
            follow(&request.root)?;
            let cancelled = || cancel.load(Ordering::Relaxed);
            ws.grep(&request, &cancelled)
                .map(Reply::Grep)
                .map_err(WireError::Message)
        }
        Call::Glob { request } => {
            follow(&request.root)?;
            ws.glob(&request)
                .map(Reply::Glob)
                .map_err(WireError::Message)
        }
        Call::ListFiles {
            root,
            show_hidden,
            max_depth,
        } => {
            follow(&root)?;
            let depth = max_depth.map(|d| usize::try_from(d).unwrap_or(usize::MAX));
            let cancelled = || cancel.load(Ordering::Relaxed);
            Ok(Reply::Files(ws.list_files(
                &root,
                show_hidden,
                depth,
                &cancelled,
            )))
        }
        // The program runs with the SSH user's rights, as a shell would; what
        // is confined is where it starts.
        Call::Exec { spec } => {
            let mut spec = ExecSpec::from(spec);
            match &spec.cwd {
                Some(cwd) => follow(cwd)?,
                None => {
                    let first = roots.first().ok_or_else(|| refused(Refusal::NoRoots))?;
                    spec.cwd = Some(first.to_string());
                }
            }
            ws.exec(spec).map(Reply::Exec).map_err(io)
        }
    }
}

/// Where a terminal starts: `cwd` when it is inside the roots, else the
/// first root.
fn pty_cwd(ws: &dyn Workspace, roots: &Roots, cwd: Option<&str>) -> Result<String, WireError> {
    let refused = |r: Refusal| WireError::Protocol(r.to_string());
    match cwd {
        Some(cwd) => {
            roots.follow(ws, cwd).map_err(refused)?;
            Ok(cwd.to_string())
        }
        None => roots
            .first()
            .map(str::to_string)
            .ok_or_else(|| refused(Refusal::NoRoots)),
    }
}

/// `path`'s canonical spelling and its subdirectories (links to directories
/// included), sorted. Names only: nothing about files.
fn browse(ws: &dyn Workspace, path: &str) -> io::Result<Browse> {
    let path = ws.canonicalize(path)?;
    if ws.metadata(&path)?.kind != EntryKind::Dir {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("{path} is not a directory"),
        ));
    }
    let child = |name: &str| {
        if path.ends_with('/') {
            format!("{path}{name}")
        } else {
            format!("{path}/{name}")
        }
    };
    let mut dirs: Vec<String> = ws
        .read_dir(&path)?
        .into_iter()
        .filter(|entry| match entry.kind {
            EntryKind::Dir => true,
            EntryKind::Symlink => ws
                .metadata(&child(&entry.name))
                .is_ok_and(|meta| meta.kind == EntryKind::Dir),
            _ => false,
        })
        .map(|entry| entry.name)
        .collect();
    dirs.sort();
    Ok(Browse { path, dirs })
}

fn read_confined(
    ws: &dyn Workspace,
    path: &str,
    request: &ConfinedRead,
) -> Result<Reply, WireError> {
    ws.read_confined(path, request)
        .map(Reply::Confined)
        .map_err(|e| WireError::from(&e))
}

fn respond(output: &Output, id: CallId, result: Result<Reply, WireError>) -> io::Result<()> {
    let body = encode(&Envelope::Response { id, result }).map_err(|e| invalid(e.to_string()))?;
    if body.len() > MAX_FRAME {
        let why = format!(
            "the reply is {} bytes, over the {MAX_FRAME}-byte frame limit",
            body.len()
        );
        return send(
            output,
            &Envelope::Response {
                id,
                result: Err(WireError::Protocol(why)),
            },
        );
    }
    write_body(output, &body)
}

fn send(output: &Output, envelope: &Envelope) -> io::Result<()> {
    let body = encode(envelope).map_err(|e| invalid(e.to_string()))?;
    write_body(output, &body)
}

fn write_body(output: &Output, body: &[u8]) -> io::Result<()> {
    let mut output = output.lock().unwrap();
    write_frame(&mut *output, body, MAX_FRAME).map_err(frame_error)
}

fn read_envelope(input: &mut impl Read) -> io::Result<Option<Envelope>> {
    match read_frame(input, MAX_FRAME).map_err(frame_error)? {
        Some(body) => decode(&body).map(Some).map_err(|e| invalid(e.to_string())),
        None => Ok(None),
    }
}

fn frame_error(error: FrameError) -> io::Error {
    match error {
        FrameError::Io(error) => error,
        other => invalid(other.to_string()),
    }
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use terax_workspace_core::env::WorkspaceEnv;
    use terax_workspace_core::host::HostWorkspace;

    #[test]
    fn a_raised_cancel_flag_reaches_the_walk() {
        let dir = tempfile::tempdir().unwrap();
        let ws = HostWorkspace::new(WorkspaceEnv::Local);
        let root = ws.canonicalize(&dir.path().to_string_lossy()).unwrap();
        std::fs::write(format!("{root}/a.txt"), "a").unwrap();
        let roots = Roots::new(&ws, std::slice::from_ref(&root)).unwrap();
        let call = || Call::ListFiles {
            root: root.clone(),
            show_hidden: false,
            max_depth: None,
        };
        let run = |cancelled: bool| handle(&ws, &roots, None, call(), &AtomicBool::new(cancelled));
        assert!(matches!(run(false), Ok(Reply::Files(Some(_)))));
        assert_eq!(run(true), Ok(Reply::Files(None)));
    }

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Runs `read_loop` over `messages` with request 5 already in flight;
    /// returns what it wrote, what it queued and request 5's cancel flag.
    fn loop_over(messages: &[Envelope]) -> (Vec<Envelope>, Vec<CallId>, bool) {
        let mut input = Vec::new();
        for message in messages {
            write_frame(&mut input, &encode(message).unwrap(), MAX_FRAME).unwrap();
        }
        let sink = Sink::default();
        let output: Output = Arc::new(Mutex::new(Box::new(sink.clone())));
        let ws = HostWorkspace::new(WorkspaceEnv::Local);
        let flag = Arc::new(AtomicBool::new(false));
        let in_flight: InFlight = Arc::new(Mutex::new(HashMap::from([(5, flag.clone())])));
        let (jobs, queued) = mpsc::channel();
        let emit: Emit = Arc::new(|_| {});
        read_loop(
            &mut &input[..],
            &Session {
                output: &output,
                ws: &ws,
                roots: &RwLock::default(),
                watches: &crate::watch::Watches::new(Arc::new(|_| {})),
                in_flight: &in_flight,
                jobs: &jobs,
                ptys: &Ptys::default(),
                home: None,
                emit: &emit,
            },
        )
        .unwrap();
        let written = sink.0.lock().unwrap().clone();
        let mut reader = &written[..];
        let mut out = Vec::new();
        while let Some(body) = read_frame(&mut reader, MAX_FRAME).unwrap() {
            out.push(decode(&body).unwrap());
        }
        (
            out,
            queued.try_iter().map(|job| job.id).collect(),
            flag.load(Ordering::Relaxed),
        )
    }

    #[test]
    fn a_second_request_with_an_id_in_flight_is_refused_not_run() {
        let call = || Call::Metadata { path: "/x".into() };
        let (out, queued, _) = loop_over(&[
            Envelope::Request {
                id: 5,
                call: call(),
            },
            Envelope::Request {
                id: 6,
                call: call(),
            },
        ]);
        assert_eq!(queued, [6]);
        assert!(matches!(
            &out[..],
            [Envelope::Response {
                id: 5,
                result: Err(WireError::Protocol(_))
            }]
        ));
    }

    #[test]
    fn cancel_raises_the_flag_of_the_request_it_names() {
        let (_, _, raised) = loop_over(&[Envelope::Cancel { id: 6 }]);
        assert!(!raised);
        let (out, _, raised) = loop_over(&[Envelope::Cancel { id: 5 }]);
        assert!(raised);
        assert!(out.is_empty(), "a cancel is not answered itself");
    }
}
