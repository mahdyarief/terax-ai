use std::io::Read;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use serde::Serialize;
use shared_child::SharedChild;

use super::ringbuffer::BoundedRingBuffer;
use crate::modules::workspace::{resolve_path, WorkspaceEnv};
use terax_remote_protocol::client::{PtyEvent, RemoteWorkspace};
use terax_remote_protocol::message::{PtyId, PtySpec};

const RING_CAP: usize = 4 * 1024 * 1024;
const THREAD_STACK: usize = 128 * 1024;

fn spawn_thread(name: &str, work: impl FnOnce() + Send + 'static) -> Result<(), String> {
    thread::Builder::new()
        .name(name.into())
        .stack_size(THREAD_STACK)
        .spawn(work)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn pump(mut pipe: impl Read, proc: &BackgroundProc) {
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => proc.buffer.lock().unwrap().push(&buf[..n]),
        }
    }
}

/// What runs a background process: a child of this machine, or a terminal
/// on a server (the protocol's PTY: its output is streamed, which a one-shot
/// `exec` is not).
enum Runner {
    Local(Arc<SharedChild>),
    Server { ws: Arc<RemoteWorkspace>, id: PtyId },
}

pub struct BackgroundProc {
    pub command: String,
    pub cwd: Option<String>,
    pub started_at_ms: u64,
    runner: Runner,
    pub buffer: Mutex<BoundedRingBuffer>,
    pub exited: AtomicBool,
    pub exit_code: AtomicI32,
    pub exit_unknown: AtomicBool,
}

#[derive(Serialize)]
pub struct BackgroundLogResponse {
    pub bytes: String,
    pub next_offset: u64,
    pub dropped: u64,
    pub exited: bool,
    pub exit_code: Option<i32>,
}

#[derive(Serialize)]
pub struct BackgroundProcInfo {
    pub handle: u32,
    pub command: String,
    pub cwd: Option<String>,
    pub started_at_ms: u64,
    pub exited: bool,
    pub exit_code: Option<i32>,
}

impl BackgroundProc {
    pub fn read_logs(&self, since: u64) -> BackgroundLogResponse {
        let (bytes, next_offset, dropped) = self.buffer.lock().unwrap().read_from(since);
        let exited = self.exited.load(Ordering::Acquire);
        let exit_code = if exited && !self.exit_unknown.load(Ordering::Acquire) {
            Some(self.exit_code.load(Ordering::Acquire))
        } else {
            None
        };
        BackgroundLogResponse {
            bytes: String::from_utf8_lossy(&bytes).into_owned(),
            next_offset,
            dropped,
            exited,
            exit_code,
        }
    }

    pub fn kill(&self) {
        match &self.runner {
            Runner::Local(child) => {
                let _ = child.kill();
            }
            // Closing the terminal hangs its process group up on the server.
            Runner::Server { ws, id } => {
                if !self.exited.load(Ordering::Acquire) {
                    let _ = ws.pty_close(*id);
                }
            }
        }
    }

    pub fn info(&self, handle: u32) -> BackgroundProcInfo {
        let exited = self.exited.load(Ordering::Acquire);
        let exit_code = if exited && !self.exit_unknown.load(Ordering::Acquire) {
            Some(self.exit_code.load(Ordering::Acquire))
        } else {
            None
        };
        BackgroundProcInfo {
            handle,
            command: self.command.clone(),
            cwd: self.cwd.clone(),
            started_at_ms: self.started_at_ms,
            exited,
            exit_code,
        }
    }
}

impl Drop for BackgroundProc {
    fn drop(&mut self) {
        self.kill();
    }
}

pub fn spawn(
    command: String,
    cwd: Option<String>,
    workspace: WorkspaceEnv,
) -> Result<Arc<BackgroundProc>, String> {
    let trimmed = command.trim().to_string();
    if trimmed.is_empty() {
        return Err("empty command".into());
    }
    if let Some(ref dir) = cwd {
        if !resolve_path(dir, &workspace).is_dir() {
            return Err(format!("cwd is not a directory: {dir}"));
        }
    }

    let mut cmd = super::build_oneshot_command(&trimmed, &workspace, cwd.as_deref())?;
    if let (WorkspaceEnv::Local, Some(ref dir)) = (&workspace, &cwd) {
        cmd.current_dir(dir);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::modules::proc::hide_console(&mut cmd);

    let shared = Arc::new(SharedChild::spawn(&mut cmd).map_err(|e| e.to_string())?);
    let kill_on_fail = || {
        let _ = shared.kill();
    };
    let stdout_pipe = shared.take_stdout().ok_or_else(|| {
        kill_on_fail();
        "no stdout pipe".to_string()
    })?;
    let stderr_pipe = shared.take_stderr().ok_or_else(|| {
        kill_on_fail();
        "no stderr pipe".to_string()
    })?;
    let child = Runner::Local(shared.clone());

    let proc = Arc::new(BackgroundProc {
        command: trimmed,
        cwd,
        started_at_ms: now_ms(),
        runner: child,
        buffer: Mutex::new(BoundedRingBuffer::new(RING_CAP)),
        exited: AtomicBool::new(false),
        exit_code: AtomicI32::new(0),
        exit_unknown: AtomicBool::new(false),
    });

    let fail = |error: String| {
        proc.kill();
        error
    };
    for (name, pipe) in [
        (
            "terax-bg-out",
            Box::new(stdout_pipe) as Box<dyn Read + Send>,
        ),
        (
            "terax-bg-err",
            Box::new(stderr_pipe) as Box<dyn Read + Send>,
        ),
    ] {
        let proc_ref = proc.clone();
        spawn_thread(name, move || pump(pipe, &proc_ref)).map_err(fail)?;
    }
    {
        let proc_ref = proc.clone();
        let child_for_wait = shared;
        spawn_thread("terax-bg-wait", move || {
            match child_for_wait.wait() {
                Ok(status) => match status.code() {
                    Some(code) => proc_ref.exit_code.store(code, Ordering::Release),
                    None => proc_ref.exit_unknown.store(true, Ordering::Release),
                },
                Err(_) => proc_ref.exit_unknown.store(true, Ordering::Release),
            }
            proc_ref.exited.store(true, Ordering::Release);
        })
        .map_err(fail)?;
    }

    Ok(proc)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A background process on a server: `sh -c` in a terminal of its own,
/// started in `cwd` (already authorized). Its output is the terminal's,
/// with line ends turned back to `\n`; a dropped connection ends it.
pub fn spawn_on_server(
    command: String,
    cwd: String,
    ws: Arc<RemoteWorkspace>,
) -> Result<Arc<BackgroundProc>, String> {
    let trimmed = command.trim().to_string();
    if trimmed.is_empty() {
        return Err("empty command".into());
    }
    let (id, _program, events) = ws
        .pty_open(PtySpec {
            cwd: Some(cwd.clone()),
            cols: 200,
            rows: 50,
            command: Some(vec!["sh".into(), "-c".into(), trimmed.clone()]),
            // Plain output for a log nobody renders as a terminal.
            env: vec![
                ("TERM".into(), "dumb".into()),
                ("NO_COLOR".into(), "1".into()),
            ],
            blocks: false,
        })
        .map_err(|e| e.to_string())?;
    let proc = Arc::new(BackgroundProc {
        command: trimmed,
        cwd: Some(cwd),
        started_at_ms: now_ms(),
        runner: Runner::Server { ws: ws.clone(), id },
        buffer: Mutex::new(BoundedRingBuffer::new(RING_CAP)),
        exited: AtomicBool::new(false),
        exit_code: AtomicI32::new(0),
        exit_unknown: AtomicBool::new(false),
    });
    let proc_ref = proc.clone();
    let pumped = spawn_thread("terax-bg-server", move || {
        let mut lines = LineEnds::default();
        for event in events {
            match event {
                PtyEvent::Data(bytes) => {
                    let _ = ws.pty_credit(id, bytes.len() as u64);
                    // ConPTY (the agent on Windows, in tests) asks where the
                    // cursor is before it starts the program.
                    if contains(&bytes, CURSOR_QUERY) {
                        let _ = ws.pty_write(id, b"\x1b[1;1R");
                    }
                    let text = lines.feed(&bytes);
                    proc_ref.buffer.lock().unwrap().push(&text);
                }
                PtyEvent::Exit { code, reason } => {
                    match code {
                        Some(code) => proc_ref.exit_code.store(code as i32, Ordering::Release),
                        None => {
                            if let Some(reason) = reason {
                                let note = format!("\n[{reason}]\n");
                                proc_ref.buffer.lock().unwrap().push(note.as_bytes());
                            }
                            proc_ref.exit_unknown.store(true, Ordering::Release);
                        }
                    }
                    break;
                }
            }
        }
        proc_ref.exited.store(true, Ordering::Release);
    });
    if let Err(error) = pumped {
        proc.kill();
        return Err(error);
    }
    Ok(proc)
}

const CURSOR_QUERY: &[u8] = b"\x1b[6n";

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// A terminal's `\r\n` as `\n`, across chunk boundaries; the cursor query
/// is dropped.
#[derive(Default)]
struct LineEnds {
    pending_cr: bool,
}

impl LineEnds {
    fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len() + 1);
        if std::mem::take(&mut self.pending_cr) && bytes.first() != Some(&b'\n') {
            out.push(b'\r');
        }
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i..].starts_with(CURSOR_QUERY) {
                i += CURSOR_QUERY.len();
                continue;
            }
            match bytes[i] {
                b'\r' if i + 1 == bytes.len() => self.pending_cr = true,
                b'\r' if bytes[i + 1] == b'\n' => {}
                byte => out.push(byte),
            }
            i += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::LineEnds;

    #[test]
    fn a_terminals_line_ends_become_newlines_across_chunks() {
        let mut lines = LineEnds::default();
        let mut out = lines.feed(b"a\r\nb\r");
        out.extend(lines.feed(b"\nc\x1b[6n\rd\r"));
        out.extend(lines.feed(b"e"));
        assert_eq!(out, b"a\nb\nc\rd\re");
    }
}
