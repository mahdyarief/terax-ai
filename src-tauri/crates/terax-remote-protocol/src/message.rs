//! What travels inside a frame: one `Envelope` per frame, MessagePack-encoded.
//!
//! `Call` carries one variant per `Workspace` operation, and `Reply` the
//! matching result. Core types cross as they are; the few that cannot
//! (`ExecSpec` holds `OsString`s and a `Duration`, errors hold `io::Error`)
//! have a wire form here.

use std::io;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::handshake::Hello;
use terax_workspace_core::confined::{ConfinedError, ConfinedOutcome, ConfinedRead};
use terax_workspace_core::exec::capture::{Captured, Limits};
use terax_workspace_core::exec::ExecSpec;
use terax_workspace_core::grep::{GlobRequest, GlobResponse, GrepRequest, GrepResponse};
use terax_workspace_core::index::FileIndex;
use terax_workspace_core::workspace::{DirEntry, Meta, ReadOutcome, ReadRequest, WriteOutcome};

/// Matches a `Response` (and a `Cancel`) to its `Request`.
pub type CallId = u64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Envelope {
    /// Each side's first message; see `handshake`.
    Hello(Hello),
    Request { id: CallId, call: Call },
    Response {
        id: CallId,
        result: Result<Reply, WireError>,
    },
    /// Asks the agent to stop working on a request; it still answers it.
    Cancel { id: CallId },
    Ping { nonce: u64 },
    Pong { nonce: u64 },
    /// From the agent, unasked: watched directories whose listing changed
    /// and watched files whose stamp moved (debounced), as `SetWatch` named
    /// them in canonical form.
    Changed { paths: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Call {
    /// The directories this session may touch; every later path is checked
    /// against them on the agent.
    SetRoots { roots: Vec<String> },
    Metadata { path: String },
    SymlinkMetadata { path: String },
    Canonicalize { path: String },
    ReadLink { path: String },
    ReadDir { path: String },
    Read { path: String, request: ReadRequest },
    ReadConfined { path: String, request: ConfinedRead },
    WriteAtomic {
        path: String,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
    CreateFile { path: String },
    CreateDirAll { path: String },
    Rename { from: String, to: String },
    ReplaceFile { from: String, to: String },
    RemoveFile { path: String },
    RemoveDirAll { path: String },
    CopyFile { from: String, to: String },
    Grep { request: GrepRequest },
    Glob { request: GlobRequest },
    ListFiles {
        root: String,
        show_hidden: bool,
        max_depth: Option<u64>,
    },
    Exec { spec: WireExecSpec },
    /// The agent user's home directory, canonical. Outside the roots: it is
    /// where a folder picker starts.
    Home,
    /// A directory's canonical path and the names of its subdirectories,
    /// outside the roots, as a folder dialog shows them: no file names, no
    /// contents. What a remote folder is picked (and authorized) with.
    BrowseDirs { path: String },
    /// Replaces the whole watched set: directories (inside the roots) and
    /// files (whose parent is inside them). Empty stops watching. Changes
    /// arrive as `Envelope::Changed`.
    SetWatch {
        dirs: Vec<String>,
        files: Vec<String>,
    },
}

/// What `BrowseDirs` returns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Browse {
    pub path: String,
    /// Sorted; links to directories included, hidden ones too.
    pub dirs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Reply {
    Unit,
    Meta(Meta),
    Path(String),
    Entries(Vec<DirEntry>),
    Read(ReadOutcome),
    Confined(ConfinedOutcome),
    Written(WriteOutcome),
    Copied(u64),
    Grep(GrepResponse),
    Glob(GlobResponse),
    /// `None` when the walk was cancelled.
    Files(Option<FileIndex>),
    Exec(Captured),
    Browse(Browse),
}

/// An error from the agent, rebuilt on the client as the same kind of error
/// with the same text, so callers keep producing today's messages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireError {
    Io { kind: WireErrorKind, message: String },
    Confined(WireConfinedError),
    /// Search engines report errors as text (`Result<_, String>`).
    Message(String),
    /// The request itself was wrong: before the handshake, outside the roots
    /// it may touch, or not understood.
    Protocol(String),
}

impl WireError {
    pub fn from_io(error: &io::Error) -> Self {
        Self::Io {
            kind: WireErrorKind::of(error.kind()),
            message: error.to_string(),
        }
    }

    /// The `io::Error` a caller of the `Workspace` trait sees.
    pub fn into_io(self) -> io::Error {
        match self {
            Self::Io { kind, message } => io::Error::new(kind.to_io(), message),
            Self::Confined(error) => {
                io::Error::new(io::ErrorKind::PermissionDenied, error.to_string())
            }
            Self::Message(message) => io::Error::other(message),
            Self::Protocol(message) => io::Error::new(io::ErrorKind::PermissionDenied, message),
        }
    }
}

impl From<&ConfinedError> for WireError {
    fn from(error: &ConfinedError) -> Self {
        match error {
            ConfinedError::Io(error) => Self::from_io(error),
            ConfinedError::NotAFile => Self::Confined(WireConfinedError::NotAFile),
            ConfinedError::Inexact => Self::Confined(WireConfinedError::Inexact),
            ConfinedError::Outside => Self::Confined(WireConfinedError::Outside),
            ConfinedError::Refused(why) => Self::Confined(WireConfinedError::Refused(why.clone())),
        }
    }
}

impl WireError {
    /// The `ConfinedError` a caller of `read_confined` sees.
    pub fn into_confined(self) -> ConfinedError {
        match self {
            Self::Confined(WireConfinedError::NotAFile) => ConfinedError::NotAFile,
            Self::Confined(WireConfinedError::Inexact) => ConfinedError::Inexact,
            Self::Confined(WireConfinedError::Outside) => ConfinedError::Outside,
            Self::Confined(WireConfinedError::Refused(why)) => ConfinedError::Refused(why),
            // A path outside the session's roots is outside every root.
            Self::Protocol(_) => ConfinedError::Outside,
            other => ConfinedError::Io(other.into_io()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireConfinedError {
    NotAFile,
    Inexact,
    Outside,
    Refused(String),
}

impl std::fmt::Display for WireConfinedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAFile => f.write_str("not a regular file"),
            Self::Inexact => f.write_str("path cannot be represented exactly"),
            Self::Outside => f.write_str("outside the allowed roots"),
            Self::Refused(why) => f.write_str(why),
        }
    }
}

macro_rules! error_kinds {
    ($($kind:ident),* $(,)?) => {
        /// `io::ErrorKind` by name, so both ends agree whatever their Rust
        /// version; a kind this side does not know arrives as `Other`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        pub enum WireErrorKind {
            $($kind,)*
            #[serde(other)]
            Other,
        }

        impl WireErrorKind {
            pub fn of(kind: io::ErrorKind) -> Self {
                match kind {
                    $(io::ErrorKind::$kind => Self::$kind,)*
                    _ => Self::Other,
                }
            }

            pub fn to_io(self) -> io::ErrorKind {
                match self {
                    $(Self::$kind => io::ErrorKind::$kind,)*
                    Self::Other => io::ErrorKind::Other,
                }
            }
        }

        #[cfg(test)]
        const ALL_KINDS: &[io::ErrorKind] = &[$(io::ErrorKind::$kind,)*];
    };
}

error_kinds!(
    NotFound,
    PermissionDenied,
    ConnectionRefused,
    ConnectionReset,
    ConnectionAborted,
    NotConnected,
    AddrInUse,
    BrokenPipe,
    AlreadyExists,
    WouldBlock,
    NotADirectory,
    IsADirectory,
    DirectoryNotEmpty,
    ReadOnlyFilesystem,
    StaleNetworkFileHandle,
    InvalidInput,
    InvalidData,
    TimedOut,
    WriteZero,
    StorageFull,
    FileTooLarge,
    ResourceBusy,
    Deadlock,
    CrossesDevices,
    TooManyLinks,
    InvalidFilename,
    ArgumentListTooLong,
    Interrupted,
    Unsupported,
    UnexpectedEof,
    OutOfMemory,
);

/// `ExecSpec` as it crosses the wire. Arguments and the environment are
/// text: the remote side is Linux, where they arrive as UTF-8 from the app
/// anyway, and a name that is not valid Unicode is refused rather than
/// approximated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireExecSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    #[serde(with = "serde_bytes")]
    pub stdin: Option<Vec<u8>>,
    pub timeout_ms: u64,
    pub limits: Limits,
}

impl TryFrom<&ExecSpec> for WireExecSpec {
    type Error = io::Error;

    fn try_from(spec: &ExecSpec) -> io::Result<Self> {
        let text = |value: &std::ffi::OsStr| {
            value.to_str().map(str::to_owned).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a remote command's arguments and environment must be valid Unicode",
                )
            })
        };
        Ok(Self {
            program: spec.program.clone(),
            args: spec.args.iter().map(|arg| text(arg)).collect::<io::Result<_>>()?,
            cwd: spec.cwd.clone(),
            env: spec
                .env
                .iter()
                .map(|(key, value)| Ok((text(key)?, text(value)?)))
                .collect::<io::Result<_>>()?,
            env_remove: spec.env_remove.iter().map(|key| text(key)).collect::<io::Result<_>>()?,
            stdin: spec.stdin.clone(),
            timeout_ms: u64::try_from(spec.timeout.as_millis()).unwrap_or(u64::MAX),
            limits: spec.limits,
        })
    }
}

impl From<WireExecSpec> for ExecSpec {
    fn from(spec: WireExecSpec) -> Self {
        Self {
            program: spec.program,
            args: spec.args.into_iter().map(Into::into).collect(),
            cwd: spec.cwd,
            env: spec
                .env
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
            env_remove: spec.env_remove.into_iter().map(Into::into).collect(),
            stdin: spec.stdin,
            timeout: Duration::from_millis(spec.timeout_ms),
            limits: spec.limits,
        }
    }
}

#[derive(Debug)]
pub enum CodecError {
    Encode(String),
    Decode(String),
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(why) => write!(f, "cannot encode message: {why}"),
            Self::Decode(why) => write!(f, "cannot decode message: {why}"),
        }
    }
}

impl std::error::Error for CodecError {}

/// The body of one frame. Field names are kept (`to_vec_named`), so a message
/// stays readable in a dump and tolerant of an added optional field.
pub fn encode(envelope: &Envelope) -> Result<Vec<u8>, CodecError> {
    rmp_serde::to_vec_named(envelope).map_err(|e| CodecError::Encode(e.to_string()))
}

/// Never panics, whatever the bytes.
pub fn decode(body: &[u8]) -> Result<Envelope, CodecError> {
    rmp_serde::from_slice(body).map_err(|e| CodecError::Decode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use terax_workspace_core::grep::{GlobHit, GrepHit, GrepPattern};
    use terax_workspace_core::workspace::EntryKind;

    fn round_trip(envelope: Envelope) {
        let bytes = encode(&envelope).unwrap();
        assert_eq!(decode(&bytes).unwrap(), envelope);
    }

    fn meta() -> Meta {
        Meta {
            kind: EntryKind::File,
            len: 5,
            mtime_ms: 1_700_000_000_000,
            readonly: false,
            token: "5-1700000000000".into(),
        }
    }

    fn exec_spec() -> WireExecSpec {
        WireExecSpec {
            program: "git".into(),
            args: vec!["status".into(), "--porcelain=v2".into()],
            cwd: Some("/home/me/repo".into()),
            env: vec![("GIT_OPTIONAL_LOCKS".into(), "0".into())],
            env_remove: vec!["GIT_DIR".into()],
            stdin: Some(b"input".to_vec()),
            timeout_ms: 30_000,
            limits: Limits {
                stdout: 1 << 20,
                stderr: 1 << 16,
                stop_on_stdout_cap: true,
            },
        }
    }

    /// One of each `Call`; adding a variant without adding it here fails the
    /// exhaustive match below.
    fn every_call() -> Vec<Call> {
        let path = || "/home/me/repo/a.txt".to_string();
        let calls = vec![
            Call::SetRoots {
                roots: vec!["/home/me/repo".into()],
            },
            Call::Metadata { path: path() },
            Call::SymlinkMetadata { path: path() },
            Call::Canonicalize { path: path() },
            Call::ReadLink { path: path() },
            Call::ReadDir { path: path() },
            Call::Read {
                path: path(),
                request: ReadRequest {
                    offset: 3,
                    limit: 1024,
                },
            },
            Call::ReadConfined {
                path: path(),
                request: ConfinedRead {
                    roots: vec![terax_workspace_core::confined::Confine {
                        path: "/home/me/repo".into(),
                        subtree: true,
                    }],
                    secret_policy: true,
                    offset: 0,
                    limit: 32 * 1024,
                },
            },
            Call::WriteAtomic {
                path: path(),
                bytes: vec![0, 159, 146, 150, 255],
            },
            Call::CreateFile { path: path() },
            Call::CreateDirAll { path: path() },
            Call::Rename {
                from: path(),
                to: "/home/me/repo/b.txt".into(),
            },
            Call::ReplaceFile {
                from: path(),
                to: "/home/me/repo/b.txt".into(),
            },
            Call::RemoveFile { path: path() },
            Call::RemoveDirAll { path: path() },
            Call::CopyFile {
                from: path(),
                to: "/home/me/repo/c.txt".into(),
            },
            Call::Grep {
                request: GrepRequest {
                    root: "/home/me/repo".into(),
                    pattern: GrepPattern::Regex {
                        pattern: "fn \\w+".into(),
                        case_insensitive: true,
                    },
                    globs: vec!["*.rs".into()],
                    max_results: Some(200),
                },
            },
            Call::Glob {
                request: GlobRequest {
                    root: "/home/me/repo".into(),
                    pattern: "**/*.md".into(),
                    max_results: None,
                },
            },
            Call::ListFiles {
                root: "/home/me/repo".into(),
                show_hidden: false,
                max_depth: Some(12),
            },
            Call::Exec { spec: exec_spec() },
            Call::Home,
            Call::BrowseDirs { path: path() },
            Call::SetWatch {
                dirs: vec![path()],
                files: vec![format!("{}/a.rs", path())],
            },
        ];
        for call in &calls {
            match call {
                Call::SetRoots { .. }
                | Call::Metadata { .. }
                | Call::SymlinkMetadata { .. }
                | Call::Canonicalize { .. }
                | Call::ReadLink { .. }
                | Call::ReadDir { .. }
                | Call::Read { .. }
                | Call::ReadConfined { .. }
                | Call::WriteAtomic { .. }
                | Call::CreateFile { .. }
                | Call::CreateDirAll { .. }
                | Call::Rename { .. }
                | Call::ReplaceFile { .. }
                | Call::RemoveFile { .. }
                | Call::RemoveDirAll { .. }
                | Call::CopyFile { .. }
                | Call::Grep { .. }
                | Call::Glob { .. }
                | Call::ListFiles { .. }
                | Call::Exec { .. }
                | Call::Home
                | Call::BrowseDirs { .. }
                | Call::SetWatch { .. } => {}
            }
        }
        calls
    }

    #[test]
    fn every_call_round_trips() {
        let calls = every_call();
        assert_eq!(calls.len(), 23);
        for (id, call) in calls.into_iter().enumerate() {
            round_trip(Envelope::Request {
                id: id as u64,
                call,
            });
        }
    }

    #[test]
    fn every_reply_round_trips() {
        let mut index = terax_workspace_core::index::Packed::default();
        assert!(index.push("src", true));
        assert!(index.push("src/main.rs", false));
        let replies = vec![
            Reply::Unit,
            Reply::Meta(meta()),
            Reply::Path("/home/me/repo".into()),
            Reply::Entries(vec![DirEntry {
                name: "a.txt".into(),
                kind: EntryKind::File,
            }]),
            Reply::Read(ReadOutcome {
                bytes: vec![1, 2, 3],
                size: 3,
                mtime_ms: 9,
            }),
            Reply::Confined(ConfinedOutcome {
                real_path: "/home/me/repo/TERAX.md".into(),
                bytes: b"memory".to_vec(),
                size: 6,
                mtime_ms: 9,
            }),
            Reply::Written(WriteOutcome {
                created: true,
                mtime_ms: 9,
            }),
            Reply::Copied(42),
            Reply::Grep(GrepResponse {
                hits: vec![GrepHit {
                    path: "/home/me/repo/a.rs".into(),
                    rel: "a.rs".into(),
                    line: 3,
                    text: "fn main() {".into(),
                }],
                truncated: false,
                files_scanned: 1,
                cancelled: false,
            }),
            Reply::Glob(GlobResponse {
                hits: vec![GlobHit {
                    path: "/home/me/repo/README.md".into(),
                    rel: "README.md".into(),
                }],
                truncated: true,
            }),
            Reply::Files(None),
            Reply::Exec(Captured {
                stdout: b"## main".to_vec(),
                stderr: Vec::new(),
                stdout_truncated: false,
                stderr_truncated: true,
                exit_code: Some(0),
                timed_out: false,
            }),
            Reply::Browse(Browse {
                path: "/home/me".into(),
                dirs: vec![".config".into(), "repo".into()],
            }),
        ];
        for (id, reply) in replies.into_iter().enumerate() {
            round_trip(Envelope::Response {
                id: id as u64,
                result: Ok(reply),
            });
        }
        // FileIndex compares through its entries.
        let files = Envelope::Response {
            id: 99,
            result: Ok(Reply::Files(Some(FileIndex {
                entries: index,
                truncated: false,
            }))),
        };
        let Envelope::Response {
            result: Ok(Reply::Files(Some(decoded))),
            ..
        } = decode(&encode(&files).unwrap()).unwrap()
        else {
            panic!("a file index reply");
        };
        assert_eq!(
            decoded.entries.iter().collect::<Vec<_>>(),
            [("src", true), ("src/main.rs", false)]
        );
        for envelope in [
            Envelope::Cancel { id: 7 },
            Envelope::Ping { nonce: 1 },
            Envelope::Pong { nonce: 1 },
            Envelope::Changed {
                paths: vec!["/srv/repo".into(), "/srv/repo/a.rs".into()],
            },
        ] {
            round_trip(envelope);
        }
    }

    #[test]
    fn bytes_travel_as_binary_not_as_arrays_of_numbers() {
        let megabyte = vec![0xAB; 1 << 20];
        let encoded = encode(&Envelope::Response {
            id: 1,
            result: Ok(Reply::Read(ReadOutcome {
                bytes: megabyte,
                size: 1 << 20,
                mtime_ms: 0,
            })),
        })
        .unwrap();
        // 0xAB as a MessagePack integer takes two bytes; as binary, one.
        assert!(encoded.len() < (1 << 20) + 256, "{} bytes", encoded.len());
    }

    #[test]
    fn every_error_kind_survives_the_trip() {
        for kind in ALL_KINDS {
            let error = io::Error::new(*kind, format!("failed: {kind:?}"));
            let wire = WireError::from_io(&error);
            let Envelope::Response {
                result: Err(decoded),
                ..
            } = decode(
                &encode(&Envelope::Response {
                    id: 1,
                    result: Err(wire),
                })
                .unwrap(),
            )
            .unwrap()
            else {
                panic!("an error response");
            };
            let rebuilt = decoded.into_io();
            assert_eq!(rebuilt.kind(), *kind);
            assert_eq!(rebuilt.to_string(), error.to_string());
        }
        // A kind with no name on the wire arrives as Other with its text.
        let odd = io::Error::new(io::ErrorKind::HostUnreachable, "no route");
        let rebuilt = WireError::from_io(&odd).into_io();
        assert_eq!(rebuilt.kind(), io::ErrorKind::Other);
        assert_eq!(rebuilt.to_string(), "no route");
    }

    #[test]
    fn an_unknown_error_kind_name_decodes_as_other() {
        #[derive(Serialize)]
        enum Future {
            SomethingNew,
        }
        let bytes = rmp_serde::to_vec_named(&Future::SomethingNew).unwrap();
        let kind: WireErrorKind = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(kind, WireErrorKind::Other);
    }

    #[test]
    fn confined_errors_keep_their_variant() {
        let cases = [
            ConfinedError::NotAFile,
            ConfinedError::Inexact,
            ConfinedError::Outside,
            ConfinedError::Refused("refused sensitive AI file".into()),
            ConfinedError::Io(io::Error::new(io::ErrorKind::NotFound, "gone")),
        ];
        for error in cases {
            let expected = format!("{error:?}");
            let wire = WireError::from(&error);
            let decoded: WireError =
                rmp_serde::from_slice(&rmp_serde::to_vec_named(&wire).unwrap()).unwrap();
            let rebuilt = decoded.into_confined();
            match (&error, &rebuilt) {
                (ConfinedError::Io(a), ConfinedError::Io(b)) => {
                    assert_eq!(a.kind(), b.kind());
                    assert_eq!(a.to_string(), b.to_string());
                }
                _ => assert_eq!(format!("{rebuilt:?}"), expected),
            }
        }
        assert!(matches!(
            WireError::Protocol("outside the session's roots".into()).into_confined(),
            ConfinedError::Outside
        ));
    }

    #[test]
    fn exec_specs_cross_and_come_back_whole() {
        let wire = exec_spec();
        let spec = ExecSpec::from(wire.clone());
        assert_eq!(spec.timeout, Duration::from_secs(30));
        assert_eq!(WireExecSpec::try_from(&spec).unwrap(), wire);
    }

    #[cfg(unix)]
    #[test]
    fn a_non_unicode_argument_is_refused_before_it_is_sent() {
        use std::os::unix::ffi::OsStringExt;
        let mut spec = ExecSpec::from(exec_spec());
        spec.args.push(std::ffi::OsString::from_vec(b"repo\xff".to_vec()));
        let error = WireExecSpec::try_from(&spec).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(windows)]
    #[test]
    fn a_non_unicode_argument_is_refused_before_it_is_sent() {
        use std::os::windows::ffi::OsStringExt;
        let mut spec = ExecSpec::from(exec_spec());
        spec.env
            .push(("X".into(), std::ffi::OsString::from_wide(&[0x72, 0xD800])));
        let error = WireExecSpec::try_from(&spec).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn a_file_index_with_bad_offsets_is_refused() {
        #[derive(Serialize)]
        struct Parts {
            buf: String,
            ends: Vec<u32>,
        }
        for ends in [vec![3, 1], vec![99], vec![2]] {
            // "aé": 'é' spans bytes 1..3, so 2 is inside it.
            let bytes = rmp_serde::to_vec_named(&Parts {
                buf: "aé".into(),
                ends: ends.clone(),
            })
            .unwrap();
            let decoded: Result<terax_workspace_core::index::Packed, _> =
                rmp_serde::from_slice(&bytes);
            assert!(decoded.is_err(), "ends {ends:?}");
        }
        let good = rmp_serde::to_vec_named(&Parts {
            buf: "aé".into(),
            ends: vec![1, 3 | (1 << 31)],
        })
        .unwrap();
        let packed: terax_workspace_core::index::Packed = rmp_serde::from_slice(&good).unwrap();
        assert_eq!(packed.iter().collect::<Vec<_>>(), [("a", false), ("é", true)]);
    }

    #[test]
    fn garbage_and_unknown_messages_are_errors_not_panics() {
        assert!(decode(&[]).is_err());
        assert!(decode(&[0xc1]).is_err(), "a reserved MessagePack byte");
        #[derive(Serialize)]
        enum Future {
            Teleport { to: String },
        }
        let unknown = rmp_serde::to_vec_named(&Future::Teleport { to: "mars".into() }).unwrap();
        assert!(decode(&unknown).is_err());
    }

    proptest! {
        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
            let _ = decode(&bytes);
        }
    }
}
