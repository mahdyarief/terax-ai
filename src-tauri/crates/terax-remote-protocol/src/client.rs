//! `RemoteWorkspace`: the `Workspace` trait served by a `terax-agent` at the
//! other end of a byte stream.
//!
//! Calls are synchronous for the caller and concurrent on the wire: each call
//! takes an id, writes its request, and waits for the reply that one reader
//! thread routes back to it. A call gives up at its deadline (and tells the
//! agent to stop). When the stream ends or breaks, every waiting call and
//! every later one fails with `ConnectionAborted`.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use terax_workspace_core::confined::{ConfinedError, ConfinedOutcome, ConfinedRead};
use terax_workspace_core::env::WorkspaceEnv;
use terax_workspace_core::exec::capture::Captured;
use terax_workspace_core::exec::ExecSpec;
use terax_workspace_core::grep::{GlobRequest, GlobResponse, GrepRequest, GrepResponse};
use terax_workspace_core::index::FileIndex;
use terax_workspace_core::workspace::{
    DirEntry, Meta, ReadOutcome, ReadRequest, Workspace, WriteOutcome,
};

use crate::frame::{read_frame, write_frame, FrameError, MAX_FRAME};
use crate::handshake::{negotiate, Hello, Session, CAP_WORKSPACE};
use crate::message::{
    decode, encode, Browse, Call, CallId, Envelope, Reply, WireError, WireExecSpec,
};

#[derive(Clone, Debug)]
pub struct Options {
    pub program: String,
    pub version: String,
    /// How long to wait for the agent's `Hello` and for `SetRoots`.
    pub connect_timeout: Duration,
    /// The deadline of a file operation.
    pub call_timeout: Duration,
    /// The deadline of a walk: grep, glob, listing files.
    pub walk_timeout: Duration,
    /// How often a waiting walk checks whether its caller cancelled it.
    pub poll: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            program: "terax".into(),
            version: String::new(),
            connect_timeout: Duration::from_secs(15),
            call_timeout: Duration::from_secs(60),
            walk_timeout: Duration::from_secs(600),
            poll: Duration::from_millis(50),
        }
    }
}

type Waiter = mpsc::Sender<Result<Reply, WireError>>;

/// What the reader thread shares with callers. It holds no writer, so
/// dropping the `RemoteWorkspace` closes the agent's input.
#[derive(Default)]
struct Shared {
    calls: Mutex<HashMap<CallId, Waiter>>,
    pings: Mutex<HashMap<u64, mpsc::Sender<()>>>,
    /// Why the connection ended; set once, before the waiters are dropped.
    closed: Mutex<Option<String>>,
}

impl Shared {
    fn close(&self, why: String) {
        self.closed.lock().unwrap().get_or_insert(why);
        // Dropping the senders wakes every waiter with `Disconnected`.
        self.calls.lock().unwrap().clear();
        self.pings.lock().unwrap().clear();
    }

    fn aborted(&self) -> io::Error {
        let why = self.closed.lock().unwrap().clone();
        io::Error::new(
            io::ErrorKind::ConnectionAborted,
            format!(
                "the connection to the agent ended: {}",
                why.unwrap_or_else(|| "closed".into())
            ),
        )
    }
}

pub struct RemoteWorkspace {
    env: WorkspaceEnv,
    session: Session,
    options: Options,
    writer: Mutex<Box<dyn Write + Send>>,
    shared: Arc<Shared>,
    next_id: AtomicU64,
}

impl RemoteWorkspace {
    /// Exchanges `Hello`s, requires the workspace capability, and sets the
    /// roots the agent will confine this session to.
    pub fn connect(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        env: WorkspaceEnv,
        roots: Vec<String>,
        options: Options,
    ) -> io::Result<Self> {
        let ours = Hello::local(&options.program, &options.version, &[CAP_WORKSPACE]);
        let mut writer: Box<dyn Write + Send> = Box::new(writer);
        send_on(&mut writer, &Envelope::Hello(ours.clone()))?;

        let shared = Arc::new(Shared::default());
        let (hello_tx, hello_rx) = mpsc::channel();
        {
            let shared = shared.clone();
            thread::Builder::new()
                .name("terax-remote-reader".into())
                .spawn(move || read_loop(reader, &shared, hello_tx))?;
        }
        let theirs = match hello_rx.recv_timeout(options.connect_timeout) {
            Ok(Ok(hello)) => hello,
            Ok(Err(error)) => return Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the agent did not answer the handshake",
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(shared.aborted()),
        };
        let session = negotiate(&ours, &theirs, &[CAP_WORKSPACE])
            .map_err(|e| io::Error::new(io::ErrorKind::Unsupported, e.to_string()))?;

        let workspace = Self {
            env,
            session,
            options,
            writer: Mutex::new(writer),
            shared,
            next_id: AtomicU64::new(1),
        };
        let deadline = workspace.options.connect_timeout;
        match workspace
            .call(Call::SetRoots { roots }, deadline, None)
            .map_err(io::Error::from)?
        {
            Reply::Unit => Ok(workspace),
            other => Err(unexpected(&other)),
        }
    }

    /// What was agreed with the agent, including its `Hello`.
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Replaces the roots the agent confines this session to (an empty list
    /// refuses every path).
    pub fn set_roots(&self, roots: Vec<String>) -> io::Result<()> {
        let deadline = self.options.connect_timeout;
        match self
            .call(Call::SetRoots { roots }, deadline, None)
            .map_err(io::Error::from)?
        {
            Reply::Unit => Ok(()),
            other => Err(unexpected(&other)),
        }
    }

    /// The agent user's home directory, canonical.
    pub fn home(&self) -> io::Result<String> {
        match self.op(Call::Home)? {
            Reply::Path(home) => Ok(home),
            other => Err(unexpected(&other)),
        }
    }

    /// A directory's canonical path and its subdirectories' names, outside
    /// the roots (what a folder picker shows).
    pub fn browse_dirs(&self, path: &str) -> io::Result<Browse> {
        match self.op(Call::BrowseDirs { path: path.into() })? {
            Reply::Browse(browse) => Ok(browse),
            other => Err(unexpected(&other)),
        }
    }

    /// Whether the stream has ended; every call fails from then on.
    pub fn is_closed(&self) -> bool {
        self.shared.closed.lock().unwrap().is_some()
    }

    /// Round-trip time to the agent, or an error if it does not answer
    /// within `timeout`.
    pub fn ping(&self, timeout: Duration) -> io::Result<Duration> {
        let nonce = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.register(|shared| shared.pings.lock().unwrap().insert(nonce, tx))?;
        let started = Instant::now();
        if let Err(error) = self.send(&Envelope::Ping { nonce }) {
            self.shared.pings.lock().unwrap().remove(&nonce);
            return Err(error);
        }
        match rx.recv_timeout(timeout) {
            Ok(()) => Ok(started.elapsed()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.shared.pings.lock().unwrap().remove(&nonce);
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the agent did not answer a ping",
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(self.shared.aborted()),
        }
    }

    /// Adds a waiter unless the connection has already ended, checked under
    /// the same lock `close` takes, so no waiter is left behind.
    fn register<T>(&self, insert: impl FnOnce(&Shared) -> T) -> io::Result<()> {
        let closed = self.shared.closed.lock().unwrap();
        if closed.is_some() {
            drop(closed);
            return Err(self.shared.aborted());
        }
        insert(&self.shared);
        drop(closed);
        Ok(())
    }

    /// Sends `call` and waits for its reply until `deadline` passes. With
    /// `cancel`, a walk polls it and asks the agent to stop when it turns
    /// true, then still waits for the (cancelled) reply.
    fn call(
        &self,
        call: Call,
        deadline: Duration,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> Result<Reply, CallError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = encode(&Envelope::Request { id, call })
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        if body.len() > MAX_FRAME {
            return Err(CallError::Transport(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "the request is {} bytes, over the {MAX_FRAME}-byte frame limit",
                    body.len()
                ),
            )));
        }
        let (tx, rx) = mpsc::channel();
        self.register(|shared| shared.calls.lock().unwrap().insert(id, tx))?;
        if let Err(error) = self.write_body(&body) {
            self.shared.calls.lock().unwrap().remove(&id);
            return Err(error.into());
        }

        let until = Instant::now() + deadline;
        let mut cancelled = false;
        loop {
            let now = Instant::now();
            if now >= until {
                self.shared.calls.lock().unwrap().remove(&id);
                let _ = self.send(&Envelope::Cancel { id });
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("the agent did not answer within {deadline:?}"),
                )
                .into());
            }
            let wait = match cancel {
                Some(_) => self.options.poll.min(until - now),
                None => until - now,
            };
            match rx.recv_timeout(wait) {
                Ok(Ok(reply)) => return Ok(reply),
                Ok(Err(error)) => return Err(CallError::Agent(error)),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(self.shared.aborted().into())
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(cancel) = cancel {
                        if !cancelled && cancel() {
                            cancelled = true;
                            let _ = self.send(&Envelope::Cancel { id });
                        }
                    }
                }
            }
        }
    }

    fn send(&self, envelope: &Envelope) -> io::Result<()> {
        let body = encode(envelope)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        self.write_body(&body)
    }

    fn write_body(&self, body: &[u8]) -> io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        write_frame(&mut *writer, body, MAX_FRAME).map_err(|error| {
            let why = error.to_string();
            self.shared.close(format!("cannot write: {why}"));
            self.shared.aborted()
        })
    }

    fn op(&self, call: Call) -> io::Result<Reply> {
        self.call(call, self.options.call_timeout, None)
            .map_err(io::Error::from)
    }
}

/// How a call failed: the agent answered with an error, or the call never
/// got an answer (deadline, broken connection, a request too large).
enum CallError {
    Agent(WireError),
    Transport(io::Error),
}

impl From<io::Error> for CallError {
    fn from(error: io::Error) -> Self {
        Self::Transport(error)
    }
}

impl From<CallError> for io::Error {
    fn from(error: CallError) -> Self {
        match error {
            CallError::Agent(error) => error.into_io(),
            CallError::Transport(error) => error,
        }
    }
}

fn unexpected(reply: &Reply) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("the agent answered with an unexpected reply: {reply:?}"),
    )
}

fn send_on(writer: &mut Box<dyn Write + Send>, envelope: &Envelope) -> io::Result<()> {
    let body =
        encode(envelope).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    write_frame(writer, &body, MAX_FRAME).map_err(frame_error)
}

fn frame_error(error: FrameError) -> io::Error {
    match error {
        FrameError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

/// Reads the agent's `Hello`, hands it over, then routes every reply to its
/// waiter until the stream ends.
fn read_loop(mut reader: impl Read, shared: &Shared, hello: mpsc::Sender<io::Result<Hello>>) {
    let next = |reader: &mut dyn Read| -> io::Result<Option<Envelope>> {
        match read_frame(&mut &mut *reader, MAX_FRAME).map_err(frame_error)? {
            Some(body) => decode(&body)
                .map(Some)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
            None => Ok(None),
        }
    };
    match next(&mut reader) {
        Ok(Some(Envelope::Hello(theirs))) => {
            let _ = hello.send(Ok(theirs));
        }
        Ok(Some(other)) => {
            let why = format!("the agent did not start with Hello: {other:?}");
            shared.close(why.clone());
            let _ = hello.send(Err(io::Error::new(io::ErrorKind::InvalidData, why)));
            return;
        }
        Ok(None) => {
            shared.close("the agent closed the stream before its Hello".into());
            return;
        }
        Err(error) => {
            shared.close(error.to_string());
            let _ = hello.send(Err(error));
            return;
        }
    }
    drop(hello);
    loop {
        match next(&mut reader) {
            Ok(Some(Envelope::Response { id, result })) => {
                // Absent when the caller gave up (deadline); dropped then.
                if let Some(waiter) = shared.calls.lock().unwrap().remove(&id) {
                    let _ = waiter.send(result);
                }
            }
            Ok(Some(Envelope::Pong { nonce })) => {
                if let Some(waiter) = shared.pings.lock().unwrap().remove(&nonce) {
                    let _ = waiter.send(());
                }
            }
            Ok(Some(other)) => {
                shared.close(format!("unexpected message from the agent: {other:?}"));
                return;
            }
            Ok(None) => {
                shared.close("the agent closed the stream".into());
                return;
            }
            Err(error) => {
                shared.close(error.to_string());
                return;
            }
        }
    }
}

macro_rules! expect {
    ($reply:expr, $variant:ident) => {
        match $reply {
            Reply::$variant(value) => Ok(value),
            other => Err(unexpected(&other)),
        }
    };
}

fn unit(reply: Reply) -> io::Result<()> {
    match reply {
        Reply::Unit => Ok(()),
        other => Err(unexpected(&other)),
    }
}

impl Workspace for RemoteWorkspace {
    fn env(&self) -> &WorkspaceEnv {
        &self.env
    }

    fn metadata(&self, path: &str) -> io::Result<Meta> {
        let reply = self.op(Call::Metadata { path: path.into() })?;
        expect!(reply, Meta)
    }

    fn symlink_metadata(&self, path: &str) -> io::Result<Meta> {
        let reply = self.op(Call::SymlinkMetadata { path: path.into() })?;
        expect!(reply, Meta)
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        let reply = self.op(Call::Canonicalize { path: path.into() })?;
        expect!(reply, Path)
    }

    fn read_link(&self, path: &str) -> io::Result<String> {
        let reply = self.op(Call::ReadLink { path: path.into() })?;
        expect!(reply, Path)
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        let reply = self.op(Call::ReadDir { path: path.into() })?;
        expect!(reply, Entries)
    }

    fn read(&self, path: &str, request: ReadRequest) -> io::Result<ReadOutcome> {
        let reply = self.op(Call::Read {
            path: path.into(),
            request,
        })?;
        expect!(reply, Read)
    }

    fn read_confined(
        &self,
        path: &str,
        request: &ConfinedRead,
    ) -> Result<ConfinedOutcome, ConfinedError> {
        let call = Call::ReadConfined {
            path: path.into(),
            request: request.clone(),
        };
        match self.call(call, self.options.call_timeout, None) {
            Ok(Reply::Confined(outcome)) => Ok(outcome),
            Ok(other) => Err(ConfinedError::Io(unexpected(&other))),
            Err(CallError::Agent(error)) => Err(error.into_confined()),
            Err(CallError::Transport(error)) => Err(ConfinedError::Io(error)),
        }
    }

    fn write_atomic(&self, path: &str, bytes: &[u8]) -> io::Result<WriteOutcome> {
        let call = Call::WriteAtomic {
            path: path.into(),
            bytes: bytes.to_vec(),
        };
        let reply = self.op(call)?;
        expect!(reply, Written)
    }

    fn create_file(&self, path: &str) -> io::Result<()> {
        unit(self.op(Call::CreateFile { path: path.into() })?)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        unit(self.op(Call::CreateDirAll { path: path.into() })?)
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        unit(self.op(Call::Rename {
            from: from.into(),
            to: to.into(),
        })?)
    }

    fn replace_file(&self, from: &str, to: &str) -> io::Result<()> {
        unit(self.op(Call::ReplaceFile {
            from: from.into(),
            to: to.into(),
        })?)
    }

    fn remove_file(&self, path: &str) -> io::Result<()> {
        unit(self.op(Call::RemoveFile { path: path.into() })?)
    }

    fn remove_dir_all(&self, path: &str) -> io::Result<()> {
        unit(self.op(Call::RemoveDirAll { path: path.into() })?)
    }

    fn copy_file(&self, from: &str, to: &str) -> io::Result<u64> {
        let reply = self.op(Call::CopyFile {
            from: from.into(),
            to: to.into(),
        })?;
        expect!(reply, Copied)
    }

    /// Uploading from this machine is a later phase's work.
    fn import(
        &self,
        _source: &Path,
        _dest_dir: &str,
        _rename_on_conflict: bool,
    ) -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "importing into a remote workspace is not supported yet",
        ))
    }

    fn grep(
        &self,
        request: &GrepRequest,
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<GrepResponse, String> {
        // Cancelled before it starts: answered here, as the host would.
        if cancel() {
            return Ok(GrepResponse {
                hits: Vec::new(),
                truncated: false,
                files_scanned: 0,
                cancelled: true,
            });
        }
        let call = Call::Grep {
            request: request.clone(),
        };
        match self.call(call, self.options.walk_timeout, Some(cancel)) {
            Ok(Reply::Grep(response)) => Ok(response),
            Ok(other) => Err(unexpected(&other).to_string()),
            // A search error (`WireError::Message`) keeps its text as is.
            Err(CallError::Agent(error)) => Err(error.into_io().to_string()),
            Err(CallError::Transport(error)) => Err(error.to_string()),
        }
    }

    fn glob(&self, request: &GlobRequest) -> Result<GlobResponse, String> {
        let call = Call::Glob {
            request: request.clone(),
        };
        match self.call(call, self.options.walk_timeout, None) {
            Ok(Reply::Glob(response)) => Ok(response),
            Ok(other) => Err(unexpected(&other).to_string()),
            // A search error (`WireError::Message`) keeps its text as is.
            Err(CallError::Agent(error)) => Err(error.into_io().to_string()),
            Err(CallError::Transport(error)) => Err(error.to_string()),
        }
    }

    /// `None` when cancelled, and also when the listing fails on the way:
    /// the trait has no error to give, and the finder shows no files either
    /// way.
    fn list_files(
        &self,
        root: &str,
        show_hidden: bool,
        max_depth: Option<usize>,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<FileIndex> {
        if cancelled() {
            return None;
        }
        let call = Call::ListFiles {
            root: root.into(),
            show_hidden,
            max_depth: max_depth.map(|depth| depth as u64),
        };
        match self.call(call, self.options.walk_timeout, Some(cancelled)) {
            Ok(Reply::Files(files)) => files,
            _ => None,
        }
    }

    fn exec(&self, spec: ExecSpec) -> io::Result<Captured> {
        let deadline = spec.timeout + self.options.call_timeout;
        let wire = WireExecSpec::try_from(&spec)?;
        let reply = self
            .call(Call::Exec { spec: wire }, deadline, None)
            .map_err(io::Error::from)?;
        expect!(reply, Exec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{PipeReader, PipeWriter};
    use std::sync::atomic::AtomicBool;
    use terax_workspace_core::confined::Confine;
    use terax_workspace_core::grep::GrepPattern;
    use terax_workspace_core::workspace::EntryKind;

    /// The agent's end of the pipes, driven by hand.
    struct Fake {
        from_client: PipeReader,
        to_client: Option<PipeWriter>,
    }

    impl Fake {
        fn recv(&mut self) -> Option<Envelope> {
            read_frame(&mut self.from_client, MAX_FRAME)
                .unwrap()
                .map(|body| decode(&body).unwrap())
        }

        fn send(&mut self, envelope: &Envelope) {
            let body = encode(envelope).unwrap();
            write_frame(self.to_client.as_mut().unwrap(), &body, MAX_FRAME).unwrap();
        }

        fn request(&mut self) -> (CallId, Call) {
            match self.recv() {
                Some(Envelope::Request { id, call }) => (id, call),
                other => panic!("expected a request, got {other:?}"),
            }
        }

        fn reply(&mut self, id: CallId, result: Result<Reply, WireError>) {
            self.send(&Envelope::Response { id, result });
        }
    }

    fn agent_hello(capabilities: &[&str]) -> Hello {
        Hello::local("terax-agent", "0.0.0-fake", capabilities)
    }

    fn options() -> Options {
        Options {
            connect_timeout: Duration::from_secs(5),
            call_timeout: Duration::from_secs(5),
            walk_timeout: Duration::from_secs(5),
            poll: Duration::from_millis(5),
            ..Options::default()
        }
    }

    /// Connects through pipes; `script` plays the agent's side of the
    /// handshake and `SetRoots`.
    fn connect_with(
        options: Options,
        capabilities: &[&str],
    ) -> (io::Result<RemoteWorkspace>, Fake) {
        let (from_client, client_out) = io::pipe().unwrap();
        let (client_in, to_client) = io::pipe().unwrap();
        let mut fake = Fake {
            from_client,
            to_client: Some(to_client),
        };
        let capabilities: Vec<String> = capabilities.iter().map(|c| c.to_string()).collect();
        let handle = thread::spawn(move || {
            let caps: Vec<&str> = capabilities.iter().map(String::as_str).collect();
            let Some(Envelope::Hello(_)) = fake.recv() else {
                panic!("the client says Hello")
            };
            fake.send(&Envelope::Hello(agent_hello(&caps)));
            if caps.contains(&CAP_WORKSPACE) {
                let (id, call) = fake.request();
                assert_eq!(
                    call,
                    Call::SetRoots {
                        roots: vec!["/srv/repo".into()]
                    }
                );
                fake.reply(id, Ok(Reply::Unit));
            }
            fake
        });
        let ws = RemoteWorkspace::connect(
            client_in,
            client_out,
            WorkspaceEnv::Local,
            vec!["/srv/repo".into()],
            options,
        );
        (ws, handle.join().unwrap())
    }

    fn connect() -> (RemoteWorkspace, Fake) {
        let (ws, fake) = connect_with(options(), &[CAP_WORKSPACE]);
        (ws.unwrap(), fake)
    }

    fn meta(len: u64) -> Meta {
        Meta {
            kind: EntryKind::File,
            len,
            mtime_ms: 0,
            readonly: false,
            token: len.to_string(),
        }
    }

    #[test]
    fn connecting_shakes_hands_and_sets_the_roots() {
        let (ws, _fake) = connect();
        assert_eq!(ws.session().peer.program, "terax-agent");
        assert!(ws.session().has(CAP_WORKSPACE));
    }

    #[test]
    fn an_agent_without_the_workspace_is_refused() {
        let (ws, _fake) = connect_with(options(), &[]);
        let error = ws.err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(error.to_string().contains(CAP_WORKSPACE), "{error}");
    }

    #[test]
    fn an_agent_that_never_says_hello_times_out() {
        let (from_client, client_out) = io::pipe().unwrap();
        let (client_in, _to_client) = io::pipe().unwrap();
        let mut opts = options();
        opts.connect_timeout = Duration::from_millis(100);
        let error =
            RemoteWorkspace::connect(client_in, client_out, WorkspaceEnv::Local, vec![], opts)
                .err()
                .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        drop(from_client);
    }

    #[test]
    fn replies_reach_their_own_caller_when_they_arrive_out_of_order() {
        let (ws, mut fake) = connect();
        let ws = Arc::new(ws);
        let callers: Vec<_> = ["/srv/repo/a", "/srv/repo/bb"]
            .into_iter()
            .map(|path| {
                let ws = ws.clone();
                thread::spawn(move || ws.metadata(path).unwrap().len)
            })
            .collect();
        let first = fake.request();
        let second = fake.request();
        // Answer the later request first; each length is its path's length.
        for (id, call) in [second, first] {
            let Call::Metadata { path } = call else {
                panic!("metadata")
            };
            fake.reply(id, Ok(Reply::Meta(meta(path.len() as u64))));
        }
        let lens: Vec<u64> = callers.into_iter().map(|c| c.join().unwrap()).collect();
        assert_eq!(
            lens,
            ["/srv/repo/a".len() as u64, "/srv/repo/bb".len() as u64]
        );
    }

    #[test]
    fn a_dropped_connection_fails_the_waiting_call_and_every_later_one() {
        let (ws, mut fake) = connect();
        let ws = Arc::new(ws);
        let waiting = {
            let ws = ws.clone();
            thread::spawn(move || ws.metadata("/srv/repo/a"))
        };
        fake.request();
        drop(fake.to_client.take());
        let error = waiting.join().unwrap().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        let later = ws.read_dir("/srv/repo").unwrap_err();
        assert_eq!(later.kind(), io::ErrorKind::ConnectionAborted);
        assert!(later.to_string().contains("closed"), "{later}");
        assert_eq!(
            ws.ping(Duration::from_secs(1)).unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );
    }

    #[test]
    fn an_oversize_write_fails_before_anything_is_sent() {
        let (ws, mut fake) = connect();
        let error = ws
            .write_atomic("/srv/repo/big", &vec![0; MAX_FRAME + 1])
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let ws = Arc::new(ws);
        let next = {
            let ws = ws.clone();
            thread::spawn(move || ws.metadata("/srv/repo/a"))
        };
        let (id, call) = fake.request();
        assert_eq!(
            call,
            Call::Metadata {
                path: "/srv/repo/a".into()
            },
            "nothing came before it"
        );
        fake.reply(id, Ok(Reply::Meta(meta(1))));
        next.join().unwrap().unwrap();
    }

    #[test]
    fn a_call_past_its_deadline_times_out_and_is_cancelled() {
        let mut opts = options();
        opts.call_timeout = Duration::from_millis(100);
        let (ws, mut fake) = connect_with(opts, &[CAP_WORKSPACE]);
        let ws = ws.unwrap();
        let error = ws.metadata("/srv/repo/slow").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        let (id, _) = fake.request();
        assert_eq!(fake.recv(), Some(Envelope::Cancel { id }));
        // A late reply is dropped, and the connection still works.
        fake.reply(id, Ok(Reply::Meta(meta(0))));
        let ws = Arc::new(ws);
        let next = {
            let ws = ws.clone();
            thread::spawn(move || ws.metadata("/srv/repo/a").map(|m| m.len))
        };
        let (id, _) = fake.request();
        fake.reply(id, Ok(Reply::Meta(meta(7))));
        assert_eq!(next.join().unwrap().unwrap(), 7);
    }

    #[test]
    fn a_cancelled_grep_tells_the_agent_and_takes_its_answer() {
        let (ws, mut fake) = connect();
        let ws = Arc::new(ws);
        let stop = Arc::new(AtomicBool::new(false));
        let search = {
            let (ws, stop) = (ws.clone(), stop.clone());
            thread::spawn(move || {
                let request = GrepRequest {
                    root: "/srv/repo".into(),
                    pattern: GrepPattern::Literal { text: "x".into() },
                    globs: Vec::new(),
                    max_results: None,
                };
                ws.grep(&request, &|| stop.load(Ordering::Relaxed))
            })
        };
        let (id, _) = fake.request();
        stop.store(true, Ordering::Relaxed);
        assert_eq!(fake.recv(), Some(Envelope::Cancel { id }));
        let answer = GrepResponse {
            hits: Vec::new(),
            truncated: false,
            files_scanned: 3,
            cancelled: true,
        };
        fake.reply(id, Ok(Reply::Grep(answer.clone())));
        assert_eq!(search.join().unwrap(), Ok(answer));
    }

    #[test]
    fn a_walk_cancelled_before_it_starts_is_not_sent() {
        let (ws, mut fake) = connect();
        let request = GrepRequest {
            root: "/srv/repo".into(),
            pattern: GrepPattern::Literal { text: "x".into() },
            globs: Vec::new(),
            max_results: None,
        };
        let stopped = ws.grep(&request, &|| true).unwrap();
        assert!(stopped.cancelled && stopped.hits.is_empty());
        assert!(ws.list_files("/srv/repo", false, None, &|| true).is_none());
        let ws = Arc::new(ws);
        let next = {
            let ws = ws.clone();
            thread::spawn(move || ws.metadata("/srv/repo/a"))
        };
        let (id, call) = fake.request();
        assert_eq!(
            call,
            Call::Metadata {
                path: "/srv/repo/a".into()
            },
            "nothing came before it"
        );
        fake.reply(id, Ok(Reply::Meta(meta(1))));
        next.join().unwrap().unwrap();
    }

    #[test]
    fn the_agents_errors_come_back_as_the_same_errors() {
        let (ws, mut fake) = connect();
        let ws = Arc::new(ws);
        let run = |fake: &mut Fake, result: Result<Reply, WireError>| {
            let (id, _) = fake.request();
            fake.reply(id, result);
        };

        let call = {
            let ws = ws.clone();
            thread::spawn(move || ws.metadata("/srv/repo/x"))
        };
        let gone = io::Error::new(io::ErrorKind::NotFound, "No such file or directory");
        run(&mut fake, Err(WireError::from_io(&gone)));
        let error = call.join().unwrap().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(error.to_string(), gone.to_string());

        let call = {
            let ws = ws.clone();
            thread::spawn(move || {
                let request = ConfinedRead {
                    roots: vec![Confine {
                        path: "/srv/repo".into(),
                        subtree: true,
                    }],
                    secret_policy: true,
                    offset: 0,
                    limit: 16,
                };
                ws.read_confined("/srv/repo/.env", &request)
            })
        };
        run(
            &mut fake,
            Err(WireError::from(&ConfinedError::Refused(
                "refused sensitive AI file".into(),
            ))),
        );
        assert!(matches!(
            call.join().unwrap(),
            Err(ConfinedError::Refused(why)) if why == "refused sensitive AI file"
        ));

        let call = {
            let ws = ws.clone();
            thread::spawn(move || {
                ws.glob(&GlobRequest {
                    root: "/srv/repo".into(),
                    pattern: "[".into(),
                    max_results: None,
                })
            })
        };
        run(&mut fake, Err(WireError::Message("invalid glob".into())));
        assert_eq!(call.join().unwrap(), Err("invalid glob".to_string()));

        let call = {
            let ws = ws.clone();
            thread::spawn(move || ws.metadata("/etc/passwd"))
        };
        run(
            &mut fake,
            Err(WireError::Protocol(
                "`/etc/passwd` is outside this session's roots".into(),
            )),
        );
        assert_eq!(
            call.join().unwrap().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn ping_measures_a_round_trip() {
        let (ws, mut fake) = connect();
        let ws = Arc::new(ws);
        let ping = {
            let ws = ws.clone();
            thread::spawn(move || ws.ping(Duration::from_secs(5)))
        };
        let Some(Envelope::Ping { nonce }) = fake.recv() else {
            panic!("a ping")
        };
        fake.send(&Envelope::Pong { nonce });
        ping.join().unwrap().unwrap();
        assert_eq!(
            ws.ping(Duration::from_millis(50)).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[test]
    fn dropping_the_workspace_closes_the_agents_input() {
        let (ws, mut fake) = connect();
        drop(ws);
        assert_eq!(fake.recv(), None);
    }
}
