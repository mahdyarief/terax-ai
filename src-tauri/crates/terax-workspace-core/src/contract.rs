//! The behaviour every `Workspace` backend must show, written only against
//! the trait so the same checks run against the host backend now and the
//! remote agent later. Each check panics with a message on failure.
//!
//! `root` is a workspace path to an empty directory the caller owns.

use std::io::ErrorKind;

use crate::confined::{Confine, ConfinedError, ConfinedRead};
use crate::grep::{GlobRequest, GrepPattern, GrepRequest};
use crate::workspace::{EntryKind, ReadRequest, Workspace};

pub fn run(ws: &dyn Workspace, root: &str) {
    directories(ws, root);
    atomic_writes(ws, root);
    bounded_reads(ws, root);
    reads_refuse_what_is_not_a_file(ws, root);
    create_file_never_overwrites(ws, root);
    listing(ws, root);
    rename_copy_remove(ws, root);
    canonical_paths(ws, root);
    search_engines(ws, root);
    file_finder_walk(ws, root);
    confined_reads(ws, root);
}

fn join(root: &str, name: &str) -> String {
    format!("{root}/{name}")
}

fn kind_of(err: std::io::Result<impl std::fmt::Debug>) -> ErrorKind {
    match err {
        Ok(value) => panic!("expected an error, got {value:?}"),
        Err(e) => e.kind(),
    }
}

fn directories(ws: &dyn Workspace, root: &str) {
    let deep = join(root, "dirs/a/b");
    ws.create_dir_all(&deep).expect("create_dir_all");
    ws.create_dir_all(&deep).expect("create_dir_all is idempotent");
    assert_eq!(ws.metadata(&deep).unwrap().kind, EntryKind::Dir);
    assert_eq!(kind_of(ws.metadata(&join(root, "dirs/missing"))), ErrorKind::NotFound);
}

fn atomic_writes(ws: &dyn Workspace, root: &str) {
    let path = join(root, "write.txt");
    let first = ws.write_atomic(&path, b"first").expect("write a new file");
    assert!(first.created, "a new file reports created");
    assert!(first.mtime_ms > 0, "the write reports the file's mtime");

    let second = ws.write_atomic(&path, b"second, longer").expect("replace");
    assert!(!second.created, "a replaced file is not created");

    let meta = ws.metadata(&path).unwrap();
    assert_eq!(meta.kind, EntryKind::File);
    assert_eq!(meta.len, 14);
    assert_eq!(meta.mtime_ms, second.mtime_ms);

    let names: Vec<_> = ws.read_dir(root).unwrap().into_iter().map(|e| e.name).collect();
    assert!(
        names.iter().filter(|n| n.as_str() != "dirs" && n.as_str() != "write.txt").count() == 0,
        "no temporary file is left behind: {names:?}"
    );

    let orphan = join(root, "no/such/dir/file.txt");
    assert_eq!(kind_of(ws.write_atomic(&orphan, b"x")), ErrorKind::NotFound);
}

fn bounded_reads(ws: &dyn Workspace, root: &str) {
    let path = join(root, "ten.txt");
    ws.write_atomic(&path, b"0123456789").unwrap();
    let read = |offset, limit| ws.read(&path, ReadRequest { offset, limit }).unwrap();

    let whole = read(0, 10);
    assert_eq!(whole.bytes, b"0123456789", "exactly the limit");
    assert_eq!(whole.size, 10);
    assert!(whole.mtime_ms > 0);

    assert_eq!(read(0, 9).bytes, b"0123456789", "one byte past the limit signals more");
    assert_eq!(read(0, 3).bytes, b"0123");
    assert_eq!(read(4, 100).bytes, b"456789", "reads from the offset");
    assert!(read(50, 10).bytes.is_empty(), "an offset past the end reads nothing");
    assert_eq!(read(0, 0).bytes, b"0");

    assert_eq!(
        kind_of(ws.read(&join(root, "missing.txt"), ReadRequest { offset: 0, limit: 1 })),
        ErrorKind::NotFound
    );
}

fn reads_refuse_what_is_not_a_file(ws: &dyn Workspace, root: &str) {
    let dir = join(root, "dirs");
    assert_eq!(
        kind_of(ws.read(&dir, ReadRequest { offset: 0, limit: 10 })),
        ErrorKind::InvalidInput
    );
    let file = join(root, "plain-for-link.txt");
    ws.write_atomic(&file, b"x").unwrap();
    for not_a_link in [file.clone(), root.to_string()] {
        assert_eq!(
            ws.read_link(&not_a_link).unwrap_err().kind(),
            ErrorKind::InvalidInput,
            "read_link on {not_a_link}"
        );
    }
    assert_eq!(
        ws.read_link(&join(root, "no-such-link")).unwrap_err().kind(),
        ErrorKind::NotFound
    );
}

fn create_file_never_overwrites(ws: &dyn Workspace, root: &str) {
    let path = join(root, "created.txt");
    ws.create_file(&path).expect("create a new file");
    assert_eq!(ws.metadata(&path).unwrap().len, 0);

    ws.write_atomic(&path, b"keep").unwrap();
    assert_eq!(kind_of(ws.create_file(&path)), ErrorKind::AlreadyExists);
    let kept = ws.read(&path, ReadRequest { offset: 0, limit: 10 }).unwrap();
    assert_eq!(kept.bytes, b"keep", "a refused create leaves the file alone");
}

fn listing(ws: &dyn Workspace, root: &str) {
    let dir = join(root, "list");
    ws.create_dir_all(&join(&dir, "sub")).unwrap();
    ws.write_atomic(&join(&dir, "b.txt"), b"b").unwrap();
    ws.write_atomic(&join(&dir, "a.txt"), b"a").unwrap();

    let mut entries = ws.read_dir(&dir).unwrap();
    entries.sort_by(|x, y| x.name.cmp(&y.name));
    let seen: Vec<_> = entries.iter().map(|e| (e.name.as_str(), e.kind)).collect();
    assert_eq!(
        seen,
        [("a.txt", EntryKind::File), ("b.txt", EntryKind::File), ("sub", EntryKind::Dir)]
    );
    assert_eq!(kind_of(ws.read_dir(&join(root, "nope"))), ErrorKind::NotFound);
}

fn rename_copy_remove(ws: &dyn Workspace, root: &str) {
    let dir = join(root, "moves");
    ws.create_dir_all(&dir).unwrap();
    let from = join(&dir, "from.txt");
    let to = join(&dir, "to.txt");
    ws.write_atomic(&from, b"payload").unwrap();

    ws.rename(&from, &to).expect("rename");
    assert_eq!(kind_of(ws.metadata(&from)), ErrorKind::NotFound);
    assert_eq!(ws.metadata(&to).unwrap().len, 7);

    // Renaming onto an existing entry is the data-loss guard.
    let occupied = join(&dir, "occupied.txt");
    ws.write_atomic(&occupied, b"keep").unwrap();
    let refused = kind_of(ws.rename(&to, &occupied));
    assert!(
        matches!(refused, ErrorKind::AlreadyExists | ErrorKind::DirectoryNotEmpty),
        "rename onto an existing file must refuse, got {refused:?}"
    );
    assert_eq!(ws.read(&occupied, ReadRequest { offset: 0, limit: 10 }).unwrap().bytes, b"keep");
    assert_eq!(ws.metadata(&to).unwrap().len, 7, "the source stays put");

    // Replacing is a separate, explicit operation.
    let token = ws.symlink_metadata(&occupied).unwrap().token;
    assert_eq!(ws.symlink_metadata(&occupied).unwrap().token, token, "stable while unchanged");
    ws.write_atomic(&occupied, b"changed!").unwrap();
    assert_ne!(ws.symlink_metadata(&occupied).unwrap().token, token, "changes with the file");
    ws.replace_file(&to, &occupied).expect("replace_file");
    assert_eq!(kind_of(ws.metadata(&to)), ErrorKind::NotFound);
    assert_eq!(ws.read(&occupied, ReadRequest { offset: 0, limit: 10 }).unwrap().bytes, b"payload");
    ws.rename(&occupied, &to).unwrap();

    let copy = join(&dir, "copy.txt");
    assert_eq!(ws.copy_file(&to, &copy).expect("copy"), 7);
    let copied = ws.read(&copy, ReadRequest { offset: 0, limit: 100 }).unwrap();
    assert_eq!(copied.bytes, b"payload");

    ws.remove_file(&copy).expect("remove_file");
    assert_eq!(kind_of(ws.metadata(&copy)), ErrorKind::NotFound);
    assert_eq!(kind_of(ws.remove_file(&copy)), ErrorKind::NotFound);

    ws.create_dir_all(&join(&dir, "nested/deeper")).unwrap();
    ws.remove_dir_all(&dir).expect("remove_dir_all");
    assert_eq!(kind_of(ws.metadata(&dir)), ErrorKind::NotFound);
}

fn canonical_paths(ws: &dyn Workspace, root: &str) {
    let direct = ws.canonicalize(&join(root, "dirs/a")).expect("canonicalize");
    let roundabout = ws.canonicalize(&join(root, "dirs/a/b/..")).expect("canonicalize ..");
    assert_eq!(direct, roundabout, "`..` resolves to the same canonical path");
    assert_eq!(
        ws.metadata(&direct).unwrap().kind,
        EntryKind::Dir,
        "a canonical path is itself a usable workspace path"
    );
    assert_eq!(kind_of(ws.canonicalize(&join(root, "missing"))), ErrorKind::NotFound);
}

fn search_engines(ws: &dyn Workspace, root: &str) {
    let tree = join(root, "search");
    for dir in ["src", "skipped", ".hidden"] {
        ws.create_dir_all(&join(&tree, dir)).unwrap();
    }
    let files: [(&str, &[u8]); 6] = [
        ("src/a.rs", b"fn needle() {}\n"),
        ("src/b.txt", b"one\nneedle two\n"),
        ("src/bin.dat", b"\0needle\n"),
        ("skipped/c.txt", b"needle\n"),
        (".hidden/d.txt", b"needle\n"),
        // `.ignore` applies outside a git repository too.
        (".ignore", b"skipped/\n"),
    ];
    for (name, bytes) in files {
        ws.write_atomic(&join(&tree, name), bytes).unwrap();
    }

    let grep = |pattern: &str, globs: &[&str], max: Option<usize>, cancel: bool| {
        let request = GrepRequest {
            root: tree.clone(),
            pattern: GrepPattern::Regex { pattern: pattern.into(), case_insensitive: false },
            globs: globs.iter().map(|g| g.to_string()).collect(),
            max_results: max,
        };
        ws.grep(&request, &move || cancel)
    };
    let rels = |hits: &[crate::grep::GrepHit]| {
        let mut rels: Vec<_> = hits.iter().map(|h| h.rel.clone()).collect();
        rels.sort();
        rels
    };

    let all = grep("needle", &[], None, false).expect("grep");
    assert_eq!(
        rels(&all.hits),
        ["src/a.rs", "src/b.txt"],
        "ignored, hidden and binary files are skipped"
    );
    assert!(!all.truncated && !all.cancelled);
    let two = all.hits.iter().find(|h| h.rel == "src/b.txt").unwrap();
    assert_eq!((two.line, two.text.as_str()), (2, "needle two"));
    assert!(two.path.ends_with("src/b.txt"), "hits carry a usable path: {}", two.path);

    assert_eq!(rels(&grep("needle", &["*.rs"], None, false).unwrap().hits), ["src/a.rs"]);
    let capped = grep("needle", &[], Some(1), false).unwrap();
    assert_eq!(capped.hits.len(), 1);
    assert!(capped.truncated, "a capped search says so");
    let stopped = grep("needle", &[], None, true).unwrap();
    assert!(stopped.cancelled && stopped.hits.is_empty());
    assert!(grep("", &[], None, false).is_err(), "an empty pattern is refused");
    assert!(grep("(", &[], None, false).is_err(), "a bad regex is refused");

    let literal = GrepRequest {
        root: tree.clone(),
        pattern: GrepPattern::Literal { text: "NEEDLE()".into() },
        globs: Vec::new(),
        max_results: None,
    };
    assert!(
        ws.grep(&literal, &|| false).unwrap().hits.is_empty(),
        "a literal with capitals is case-sensitive and not a regex"
    );

    let globbed = ws
        .glob(&GlobRequest { root: tree.clone(), pattern: "**/*.txt".into(), max_results: None })
        .expect("glob");
    let mut names: Vec<_> = globbed.hits.iter().map(|h| h.rel.as_str()).collect();
    names.sort();
    assert_eq!(names, ["src/b.txt"]);
    assert!(ws
        .glob(&GlobRequest { root: join(&tree, "nope"), pattern: "*".into(), max_results: None })
        .is_err());
}

fn file_finder_walk(ws: &dyn Workspace, root: &str) {
    let tree = join(root, "walk");
    for dir in ["src/deep", "node_modules/pkg", "ignored", ".hidden"] {
        ws.create_dir_all(&join(&tree, dir)).unwrap();
    }
    for file in ["src/a.rs", "src/deep/b.rs", "node_modules/pkg/index.js", "ignored/x.txt", ".hidden/h.txt"] {
        ws.write_atomic(&join(&tree, file), b"").unwrap();
    }
    ws.write_atomic(&join(&tree, ".ignore"), b"ignored/\n").unwrap();

    let listed = |show_hidden: bool, max_depth: Option<usize>| {
        let index = ws.list_files(&tree, show_hidden, max_depth, &|| false).expect("walk");
        assert!(!index.truncated);
        let mut entries: Vec<(String, bool)> =
            index.entries.iter().map(|(rel, is_dir)| (rel.to_string(), is_dir)).collect();
        entries.sort();
        entries
    };
    let entry = |rel: &str, is_dir: bool| (rel.to_string(), is_dir);

    assert_eq!(
        listed(false, None),
        [entry("src", true), entry("src/a.rs", false), entry("src/deep", true), entry("src/deep/b.rs", false)],
        "pruned, ignored and hidden entries are left out; directories are flagged"
    );
    assert!(
        listed(true, None).contains(&entry(".hidden/h.txt", false)),
        "show_hidden lists dot entries"
    );
    assert_eq!(
        listed(false, Some(1)),
        [entry("src", true)],
        "max_depth stops the walk"
    );
    assert!(ws.list_files(&tree, false, None, &|| true).is_none(), "a cancelled walk returns nothing");
}

fn confined_reads(ws: &dyn Workspace, root: &str) {
    let base = join(root, "confined");
    ws.create_dir_all(&join(&base, "inside/sub")).unwrap();
    ws.create_dir_all(&join(&base, "outside")).unwrap();
    ws.write_atomic(&join(&base, "inside/sub/a.txt"), b"hello").unwrap();
    ws.write_atomic(&join(&base, "inside/.env"), b"SECRET").unwrap();
    ws.write_atomic(&join(&base, "outside/b.txt"), b"nope").unwrap();
    let inside = ws.canonicalize(&join(&base, "inside")).unwrap();
    let file = ws.canonicalize(&join(&base, "inside/sub/a.txt")).unwrap();
    let request = |roots: Vec<Confine>, secret_policy: bool| ConfinedRead {
        roots,
        secret_policy,
        offset: 0,
        limit: 1024,
    };
    let subtree = || {
        vec![Confine {
            path: inside.clone(),
            subtree: true,
        }]
    };

    let read = ws
        .read_confined(&join(&base, "inside/sub/a.txt"), &request(subtree(), true))
        .expect("a file inside the root");
    assert_eq!(read.bytes, b"hello");
    assert_eq!(read.size, 5);
    assert_eq!(read.real_path, file, "the real path is the canonical workspace path");

    let bounded = ws
        .read_confined(
            &join(&base, "inside/sub/a.txt"),
            &ConfinedRead {
                roots: subtree(),
                secret_policy: false,
                offset: 1,
                limit: 2,
            },
        )
        .unwrap();
    assert_eq!(bounded.bytes, b"ell", "offset, then up to limit + 1 bytes");

    assert!(matches!(
        ws.read_confined(&join(&base, "outside/b.txt"), &request(subtree(), true)),
        Err(ConfinedError::Outside)
    ));
    assert!(matches!(
        ws.read_confined(&join(&base, "inside/.env"), &request(subtree(), true)),
        Err(ConfinedError::Refused(_))
    ));
    assert!(
        ws.read_confined(&join(&base, "inside/.env"), &request(subtree(), false))
            .is_ok(),
        "the secret policy applies only when asked"
    );
    assert!(matches!(
        ws.read_confined(&join(&base, "inside/sub"), &request(subtree(), true)),
        Err(ConfinedError::NotAFile)
    ));
    assert!(matches!(
        ws.read_confined(&join(&base, "inside/missing.txt"), &request(subtree(), true)),
        Err(ConfinedError::Io(e)) if e.kind() == ErrorKind::NotFound
    ));

    let exact = || {
        vec![Confine {
            path: file.clone(),
            subtree: false,
        }]
    };
    assert!(ws
        .read_confined(&join(&base, "inside/sub/a.txt"), &request(exact(), false))
        .is_ok());
    ws.write_atomic(&join(&base, "inside/sub/a.txt.bak"), b"x").unwrap();
    assert!(
        matches!(
            ws.read_confined(&join(&base, "inside/sub/a.txt.bak"), &request(exact(), false)),
            Err(ConfinedError::Outside)
        ),
        "a file root admits that file only"
    );
}
