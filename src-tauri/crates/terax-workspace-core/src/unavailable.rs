//! A workspace that cannot be reached right now (a server with no live
//! connection). Every operation fails with `NotConnected` and the reason, so
//! a caller never falls back to anything else, least of all this machine.

use std::io;
use std::path::Path;

use crate::confined::{ConfinedError, ConfinedOutcome, ConfinedRead};
use crate::env::WorkspaceEnv;
use crate::exec::capture::Captured;
use crate::exec::ExecSpec;
use crate::grep::{GlobRequest, GlobResponse, GrepRequest, GrepResponse};
use crate::index::FileIndex;
use crate::workspace::{DirEntry, Meta, ReadOutcome, ReadRequest, Workspace, WriteOutcome};

pub struct Unavailable {
    env: WorkspaceEnv,
    why: String,
}

impl Unavailable {
    pub fn new(env: WorkspaceEnv, why: impl Into<String>) -> Self {
        Self {
            env,
            why: why.into(),
        }
    }

    fn error(&self) -> io::Error {
        io::Error::new(io::ErrorKind::NotConnected, self.why.clone())
    }

    fn fail<T>(&self) -> io::Result<T> {
        Err(self.error())
    }
}

impl Workspace for Unavailable {
    fn env(&self) -> &WorkspaceEnv {
        &self.env
    }
    fn metadata(&self, _: &str) -> io::Result<Meta> {
        self.fail()
    }
    fn symlink_metadata(&self, _: &str) -> io::Result<Meta> {
        self.fail()
    }
    fn canonicalize(&self, _: &str) -> io::Result<String> {
        self.fail()
    }
    fn read_link(&self, _: &str) -> io::Result<String> {
        self.fail()
    }
    fn read_dir(&self, _: &str) -> io::Result<Vec<DirEntry>> {
        self.fail()
    }
    fn read(&self, _: &str, _: ReadRequest) -> io::Result<ReadOutcome> {
        self.fail()
    }
    fn read_confined(&self, _: &str, _: &ConfinedRead) -> Result<ConfinedOutcome, ConfinedError> {
        Err(ConfinedError::Io(self.error()))
    }
    fn write_atomic(&self, _: &str, _: &[u8]) -> io::Result<WriteOutcome> {
        self.fail()
    }
    fn create_file(&self, _: &str) -> io::Result<()> {
        self.fail()
    }
    fn create_dir_all(&self, _: &str) -> io::Result<()> {
        self.fail()
    }
    fn rename(&self, _: &str, _: &str) -> io::Result<()> {
        self.fail()
    }
    fn replace_file(&self, _: &str, _: &str) -> io::Result<()> {
        self.fail()
    }
    fn remove_file(&self, _: &str) -> io::Result<()> {
        self.fail()
    }
    fn remove_dir_all(&self, _: &str) -> io::Result<()> {
        self.fail()
    }
    fn copy_file(&self, _: &str, _: &str) -> io::Result<u64> {
        self.fail()
    }
    fn import(&self, _: &Path, _: &str, _: bool) -> io::Result<String> {
        self.fail()
    }
    fn grep(&self, _: &GrepRequest, _: &(dyn Fn() -> bool + Sync)) -> Result<GrepResponse, String> {
        Err(self.why.clone())
    }
    fn glob(&self, _: &GlobRequest) -> Result<GlobResponse, String> {
        Err(self.why.clone())
    }
    fn list_files(
        &self,
        _: &str,
        _: bool,
        _: Option<usize>,
        _: &dyn Fn() -> bool,
    ) -> Option<FileIndex> {
        None
    }
    fn exec(&self, _: ExecSpec) -> io::Result<Captured> {
        self.fail()
    }
}
