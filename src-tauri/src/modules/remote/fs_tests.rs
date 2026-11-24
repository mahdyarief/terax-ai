//! The explorer's and the editor's commands on a server's folder, through
//! the real agent served in-process: what they do on this machine, done on
//! the server, and nothing outside the authorized folder.

use std::path::Path;

use terax_workspace_core::env::{SshHost, WorkspaceEnv};
use terax_workspace_core::workspace::Workspace;

use super::testing::connected;
use crate::modules::fs::copy::copy_batch;
use crate::modules::fs::file::{read_file_sync, ReadResult};
use crate::modules::fs::index::FileIndexState;
use crate::modules::fs::mutate::{
    create_dir, create_file, fs_move_impl, remove_path, rename, FsMoveResult,
};
use crate::modules::fs::tree::{read_dir_entries, subdirs};
use crate::modules::workspace::{backend_for, WorkspaceRegistry};

struct Remote {
    _dir: tempfile::TempDir,
    /// The authorized folder, as the server spells it.
    root: String,
    /// A sibling folder that is not authorized.
    outside: String,
    env: WorkspaceEnv,
    registry: WorkspaceRegistry,
}

/// `root/src/main.rs`, `root/README.md`, `root/.hidden/`, and a sibling
/// `outside/secret.txt`, with `root` authorized on host `name`.
fn remote(name: &str) -> Remote {
    let host: SshHost = connected(name);
    let env = WorkspaceEnv::Ssh { host };
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_string_lossy().replace('\\', "/");
    std::fs::create_dir_all(format!("{base}/root/src")).unwrap();
    std::fs::create_dir_all(format!("{base}/root/.hidden")).unwrap();
    std::fs::create_dir_all(format!("{base}/outside")).unwrap();
    std::fs::write(format!("{base}/root/src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(format!("{base}/root/README.md"), "# hi\n").unwrap();
    std::fs::write(format!("{base}/outside/secret.txt"), "no").unwrap();
    let registry = WorkspaceRegistry::default();
    let root = registry
        .authorize_in(&env, &format!("{base}/root"))
        .unwrap();
    let outside = format!("{}/outside", root.trim_end_matches("/root"));
    Remote {
        _dir: dir,
        root,
        outside,
        env,
        registry,
    }
}

impl Remote {
    fn at(&self, rel: &str) -> String {
        format!("{}/{rel}", self.root)
    }
    fn ws(&self) -> std::sync::Arc<dyn Workspace> {
        backend_for(&self.env)
    }
    fn listing(&self, dir: &str, hidden: bool) -> Vec<(String, String)> {
        let mut entries: Vec<_> =
            read_dir_entries(dir.into(), hidden, Some(true), Some(self.env.clone()))
                .unwrap()
                .into_iter()
                .map(|e| (e.name, serde_json::to_string(&e.kind).unwrap()))
                .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }
}

#[test]
fn the_explorer_lists_a_server_folder() {
    let r = remote("fs-list");
    assert_eq!(
        r.listing(&r.root, false),
        [
            ("README.md".to_string(), r#""file""#.to_string()),
            ("src".to_string(), r#""dir""#.to_string()),
        ]
    );
    assert!(r.listing(&r.root, true).iter().any(|(n, _)| n == ".hidden"));
    assert_eq!(
        subdirs(r.root.clone(), false, Some(r.env.clone())).unwrap(),
        ["src"]
    );
    // Not the authorized folder: refused by the agent.
    assert!(read_dir_entries(r.outside.clone(), false, None, Some(r.env.clone())).is_err());
}

#[test]
fn the_editor_opens_saves_and_sees_a_change_made_on_the_server() {
    let r = remote("fs-edit");
    let ws = r.ws();
    let path = r.at("src/main.rs");
    let ReadResult::Text { content, mtime, .. } = read_file_sync(&*ws, &path, false).unwrap()
    else {
        panic!("text");
    };
    assert_eq!(content, "fn main() {}\n");

    let written = ws.write_atomic(&path, b"fn main() { run() }\n").unwrap();
    assert!(!written.created);
    let ReadResult::Text {
        content,
        mtime: saved,
        ..
    } = read_file_sync(&*ws, &path, false).unwrap()
    else {
        panic!("text");
    };
    assert_eq!(content, "fn main() { run() }\n");
    assert_eq!(saved, written.mtime_ms, "the save's stamp is the file's");
    let _ = mtime;

    // Someone edits it on the server: the stamp moves, so the editor's
    // conflict check (saved stamp vs disk) sees it.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let host_path = path.clone();
    std::fs::write(&host_path, "changed elsewhere\n").unwrap();
    assert_ne!(ws.metadata(&path).unwrap().mtime_ms, saved);

    assert!(ws
        .read(
            &format!("{}/secret.txt", r.outside),
            terax_workspace_core::workspace::ReadRequest {
                offset: 0,
                limit: 10
            }
        )
        .is_err());
}

#[test]
fn files_are_created_renamed_moved_and_deleted_on_the_server() {
    let r = remote("fs-mutate");
    let ws = r.ws();
    create_file(&*ws, &r.at("new.txt")).unwrap();
    assert!(
        create_file(&*ws, &r.at("new.txt")).is_err(),
        "never replaces"
    );
    create_dir(&*ws, &r.at("docs/guides")).unwrap();
    rename(&*ws, &r.at("new.txt"), &r.at("docs/renamed.txt")).unwrap();

    // A move is a move, not a no-op: remote paths have no host spelling to
    // compare, so they are compared as spelled.
    let moved = fs_move_impl(
        &*ws,
        &r.at("docs/renamed.txt"),
        &r.at("docs/guides/moved.txt"),
        &r.root,
        None,
    )
    .unwrap();
    assert_eq!(moved, FsMoveResult::Moved);
    assert!(ws.metadata(&r.at("docs/guides/moved.txt")).is_ok());
    assert!(ws.metadata(&r.at("docs/renamed.txt")).is_err());
    // Onto an existing file: a conflict to confirm, then a replace.
    let conflict = fs_move_impl(
        &*ws,
        &r.at("README.md"),
        &r.at("docs/guides/moved.txt"),
        &r.root,
        None,
    )
    .unwrap();
    let FsMoveResult::Conflict {
        replaceable: true,
        token,
    } = conflict
    else {
        panic!("{conflict:?}");
    };
    assert_eq!(
        fs_move_impl(
            &*ws,
            &r.at("README.md"),
            &r.at("docs/guides/moved.txt"),
            &r.root,
            Some(&token)
        )
        .unwrap(),
        FsMoveResult::Moved
    );
    // Out of the folder: refused.
    assert!(fs_move_impl(
        &*ws,
        &r.at("src/main.rs"),
        &format!("{}/main.rs", r.outside),
        &r.root,
        None
    )
    .is_err());

    remove_path(&*ws, &r.at("docs")).unwrap();
    assert!(ws.metadata(&r.at("docs")).is_err());
    assert!(remove_path(&*ws, &format!("{}/secret.txt", r.outside)).is_err());
    assert!(Path::new(&format!("{}/secret.txt", r.outside)).exists());
}

#[test]
fn files_from_this_machine_are_uploaded_into_the_folder() {
    let r = remote("fs-copy");
    let local = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(local.path().join("assets/img")).unwrap();
    std::fs::write(local.path().join("assets/img/logo.svg"), "<svg/>").unwrap();
    std::fs::write(local.path().join("README.md"), "dropped").unwrap();
    let sources = vec![
        local.path().join("assets").to_string_lossy().into_owned(),
        local
            .path()
            .join("README.md")
            .to_string_lossy()
            .into_owned(),
    ];
    let index = FileIndexState::default();
    let result = copy_batch(sources, &r.root, &r.root, &r.env, &r.registry, &index, true).unwrap();
    assert!(result.failed.is_empty(), "{:?}", result.failed);
    assert_eq!(result.created.len(), 2);
    let ws = r.ws();
    assert!(ws.metadata(&r.at("assets/img/logo.svg")).is_ok());
    // README.md was there: the drop got a copy name.
    assert!(ws.metadata(&r.at("README copy.md")).is_ok());
    assert!(
        !r.listing(&r.root, true)
            .iter()
            .any(|(n, _)| n.starts_with(".terax-copy-")),
        "no staging left"
    );
    // Not into a folder that is not authorized.
    let refused = copy_batch(
        vec![local
            .path()
            .join("README.md")
            .to_string_lossy()
            .into_owned()],
        &r.outside,
        &r.outside,
        &r.env,
        &r.registry,
        &index,
        true,
    );
    assert!(refused.is_err());
}

#[test]
fn previews_canonical_paths_and_project_memory_come_from_the_server() {
    use base64::Engine;

    use crate::modules::fs::file::{
        fs_canonicalize, fs_read_binary, fs_read_head, ReadResultWithBytes,
    };
    use crate::modules::fs::grep::run_grep_interactive;
    use crate::modules::fs::project_memory::read_memory_in;

    let r = remote("fs-reads");
    let readme = r.at("README.md");
    let ReadResultWithBytes::Binary { content, size } =
        tauri::async_runtime::block_on(fs_read_binary(readme.clone(), Some(r.env.clone())))
            .unwrap()
    else {
        panic!("bytes");
    };
    assert_eq!(size, 5);
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(content)
            .unwrap(),
        b"# hi\n"
    );
    let head = tauri::async_runtime::block_on(fs_read_head(
        r.at("src/main.rs"),
        Some(r.env.clone()),
        Some(3),
        Some(true),
        None,
    ))
    .unwrap();
    assert_eq!(head.content, "fn ");
    // Resolved on the server, in its spelling.
    assert_eq!(
        tauri::async_runtime::block_on(fs_canonicalize(
            r.at("src/../README.md"),
            Some(r.env.clone()),
            None
        ))
        .unwrap(),
        readme
    );
    // A link the user clicked must be a file on this machine.
    assert!(tauri::async_runtime::block_on(fs_canonicalize(
        readme.clone(),
        Some(r.env.clone()),
        Some(true)
    ))
    .is_err());

    // The project's TERAX.md, inside the authorized folder.
    let ws = r.ws();
    assert!(r.registry.covers(&r.env, &r.root));
    assert!(!r.registry.covers(&r.env, &r.outside));
    // None yet: the frontend only asks for one it found.
    assert!(read_memory_in(&*ws, &r.root, &r.at("TERAX.md")).is_err());
    ws.write_atomic(&r.at("TERAX.md"), b"be kind").unwrap();
    assert_eq!(
        read_memory_in(&*ws, &r.root, &r.at("TERAX.md")).unwrap(),
        Some("be kind".to_string())
    );
    // Not another file than the one the frontend checked.
    assert!(read_memory_in(&*ws, &r.root, &r.at("README.md")).is_err());

    let hits = run_grep_interactive(
        "main".into(),
        r.root.clone(),
        None,
        Some(r.env.clone()),
        &|| false,
    )
    .unwrap();
    assert_eq!(hits.hits.len(), 1);
}

#[test]
fn a_pasted_image_is_uploaded_to_the_server() {
    use crate::modules::pty::paste_image::{save_on_server, FOLDER};

    let r = remote("paste-image");
    // The server user's home stands in as the authorized folder here.
    let path = save_on_server(&r.env, &r.root, b"png!", 1_795_534_212_042).unwrap();
    assert_eq!(
        path,
        format!("{}/.cache/{FOLDER}/image-20261124-153012-042.png", r.root)
    );
    // On the server, in its spelling, with the bytes pasted.
    assert_eq!(r.ws().metadata(&path).unwrap().len, 4);
    let again = save_on_server(&r.env, &r.root, b"next", 1_795_534_212_042).unwrap();
    assert_ne!(again, path);
}
