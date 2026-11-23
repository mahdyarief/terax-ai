//! Finding files and text in a server's folder: the file finder's index,
//! grep and glob, through the real agent served in-process.

use terax_workspace_core::env::WorkspaceEnv;

use super::testing::connected;
use crate::modules::fs::grep::{run_glob, run_grep};
use crate::modules::fs::index::FileIndexState;
use crate::modules::fs::mutate::note_changed_in;
use crate::modules::fs::search::{run_list_files, run_search, SearchOptions};
use crate::modules::workspace::WorkspaceRegistry;

struct Remote {
    _dir: tempfile::TempDir,
    root: String,
    env: WorkspaceEnv,
    index: FileIndexState,
}

/// A repo with `src/lib.rs`, `src/app/main.rs`, a gitignored `build/`, and
/// `node_modules/` (pruned).
fn remote(name: &str) -> Remote {
    let env = WorkspaceEnv::Ssh {
        host: connected(name),
    };
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_string_lossy().replace('\\', "/");
    for sub in [
        "root/.git",
        "root/src/app",
        "root/build",
        "root/node_modules/pkg",
    ] {
        std::fs::create_dir_all(format!("{base}/{sub}")).unwrap();
    }
    std::fs::write(format!("{base}/root/.gitignore"), "build/\n").unwrap();
    std::fs::write(format!("{base}/root/src/lib.rs"), "pub fn needle() {}\n").unwrap();
    std::fs::write(
        format!("{base}/root/src/app/main.rs"),
        "fn main() { needle() }\n",
    )
    .unwrap();
    std::fs::write(format!("{base}/root/build/needle.rs"), "needle\n").unwrap();
    std::fs::write(
        format!("{base}/root/node_modules/pkg/needle.js"),
        "needle\n",
    )
    .unwrap();
    let root = WorkspaceRegistry::default()
        .authorize_in(&env, &format!("{base}/root"))
        .unwrap();
    Remote {
        _dir: dir,
        root,
        env,
        index: FileIndexState::default(),
    }
}

impl Remote {
    fn search(&self, query: &str) -> Vec<String> {
        self.search_with(query, &|| false).unwrap()
    }
    fn search_with(
        &self,
        query: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<String>, String> {
        let options = SearchOptions {
            files_only: true,
            with_indices: false,
        };
        let result = run_search(
            &self.index,
            self.root.clone(),
            query.into(),
            None,
            Some(self.env.clone()),
            None,
            options,
            cancelled,
        )?;
        Ok(result.hits.into_iter().map(|hit| hit.path).collect())
    }
    fn files(&self) -> Vec<String> {
        run_list_files(
            &self.index,
            self.root.clone(),
            None,
            None,
            Some(self.env.clone()),
            None,
        )
        .unwrap()
        .files
    }
}

#[test]
fn the_finder_finds_files_on_the_server_and_leaves_ignored_ones_out() {
    let r = remote("search-find");
    assert_eq!(r.search("main"), [format!("{}/src/app/main.rs", r.root)]);
    // Only in the ignored and the pruned folders.
    assert!(r.search("needle").is_empty());
    // Dotfiles are hidden unless asked for, as on this machine.
    assert_eq!(r.files(), ["src/app/main.rs", "src/lib.rs"]);
}

#[test]
fn the_index_sees_our_own_changes_and_rebuilds_after_a_reconnect() {
    let r = remote("search-fresh");
    assert!(r.search("notes").is_empty());

    // Made through the app: reported to the index, which drops the cache.
    std::fs::write(format!("{}/src/notes.md", r.root), "x").unwrap();
    note_changed_in(&r.index, &r.env, &format!("{}/src/notes.md", r.root));
    assert_eq!(r.search("notes"), [format!("{}/src/notes.md", r.root)]);

    // Made on the server while nothing watched: cached until a reconnect.
    std::fs::write(format!("{}/todo.md", r.root), "x").unwrap();
    assert!(r.search("todo").is_empty(), "still the cached listing");
    let WorkspaceEnv::Ssh { host } = &r.env else {
        unreachable!()
    };
    r.index.forget_host(host);
    assert_eq!(r.search("todo"), [format!("{}/todo.md", r.root)]);
}

#[test]
fn a_cancelled_search_stops_and_a_missing_root_is_an_error() {
    let r = remote("search-cancel");
    assert!(r.search_with("main", &|| true).unwrap().is_empty());
    let missing = run_search(
        &r.index,
        format!("{}/nope", r.root),
        "main".into(),
        None,
        Some(r.env.clone()),
        None,
        SearchOptions {
            files_only: true,
            with_indices: false,
        },
        &|| false,
    );
    assert!(missing.is_err());
}

#[test]
fn grep_and_glob_run_on_the_server() {
    let r = remote("search-grep");
    let grep = run_grep(
        "needle".into(),
        r.root.clone(),
        None,
        None,
        None,
        Some(r.env.clone()),
    )
    .unwrap();
    let mut rels: Vec<_> = grep.hits.iter().map(|hit| hit.rel.clone()).collect();
    rels.sort();
    // Gitignored files are skipped; grep does not prune `node_modules`,
    // the same as on this machine.
    assert_eq!(
        rels,
        [
            "node_modules/pkg/needle.js",
            "src/app/main.rs",
            "src/lib.rs"
        ]
    );
    let glob = run_glob("**/*.rs".into(), r.root.clone(), None, Some(r.env.clone())).unwrap();
    assert_eq!(glob.hits.len(), 2);
}
