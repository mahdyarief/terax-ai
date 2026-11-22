//! Content search (grep) and file-name matching (glob) over a workspace
//! tree, honouring `.gitignore`. These run next to the data: on this machine
//! for local and WSL workspaces, on the server for remote ones, so only the
//! matches cross a connection.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use globset::{Glob, GlobSet, GlobSetBuilder};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::{WalkBuilder, WalkState};
use serde::{Deserialize, Serialize};

use crate::env::WorkspaceEnv;
use crate::paths::{display_path, to_canon};

const FILE_SIZE_CAP: u64 = 5 * 1024 * 1024;
pub const DEFAULT_MAX_RESULTS: usize = 200;
pub const HARD_MAX_RESULTS: usize = 2000;

/// Walker threads per search. The default is one per core, which for a burst of
/// keystrokes spawned a dozen threads per query.
const WALK_THREADS: usize = 4;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrepHit {
    pub path: String,
    pub rel: String,
    pub line: u64,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrepResponse {
    pub hits: Vec<GrepHit>,
    pub truncated: bool,
    pub files_scanned: usize,
    /// Stopped early because a newer query replaced this one.
    pub cancelled: bool,
}

fn build_globset(patterns: &[String]) -> Result<Option<GlobSet>, String> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let g = Glob::new(p).map_err(|e| format!("bad glob {p:?}: {e}"))?;
        b.add(g);
    }
    let set = b.build().map_err(|e| format!("globset build: {e}"))?;
    Ok(Some(set))
}

fn escape_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn search_tree(
    root_path: &Path,
    root_display: &str,
    workspace: &WorkspaceEnv,
    matcher: &RegexMatcher,
    globs: &Option<GlobSet>,
    cap: usize,
    cancel: &(dyn Fn() -> bool + Sync),
) -> GrepResponse {
    let walker = WalkBuilder::new(root_path)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .follow_links(false)
        .threads(WALK_THREADS)
        .build_parallel();

    let hits: Arc<Mutex<Vec<GrepHit>>> = Arc::new(Mutex::new(Vec::new()));
    let scanned = Arc::new(AtomicUsize::new(0));
    let truncated = Arc::new(AtomicBool::new(false));

    walker.run(|| {
        let matcher = matcher.clone();
        let globs = globs.clone();
        let hits = hits.clone();
        let scanned = scanned.clone();
        let truncated = truncated.clone();
        let root_path = root_path.to_path_buf();
        let root_display = root_display.to_string();
        let workspace = workspace.clone();

        Box::new(move |dent_res| {
            if truncated.load(Ordering::Relaxed) || cancel() {
                return WalkState::Quit;
            }
            let dent = match dent_res {
                Ok(d) => d,
                Err(_) => return WalkState::Continue,
            };
            if !dent.file_type().map(|t| t.is_file()).unwrap_or(false) {
                return WalkState::Continue;
            }
            let path = dent.path();
            let rel = match path.strip_prefix(&root_path) {
                Ok(r) => to_canon(r),
                Err(_) => return WalkState::Continue,
            };
            if let Some(set) = globs.as_ref() {
                if !set.is_match(&rel) {
                    return WalkState::Continue;
                }
            }
            if let Ok(meta) = std::fs::metadata(path) {
                if meta.len() > FILE_SIZE_CAP {
                    return WalkState::Continue;
                }
            }

            scanned.fetch_add(1, Ordering::Relaxed);

            let abs = display_path(path, &root_path, &root_display, &workspace);
            let rel_clone = rel.clone();
            let mut searcher = SearcherBuilder::new()
                .binary_detection(BinaryDetection::quit(b'\x00'))
                .line_number(true)
                .build();

            let _ = searcher.search_path(
                &matcher,
                path,
                UTF8(|line_num, text| {
                    let line_text = text.trim_end_matches('\n').to_string();
                    let mut guard = hits.lock().unwrap();
                    if guard.len() >= cap {
                        truncated.store(true, Ordering::Relaxed);
                        return Ok(false);
                    }
                    guard.push(GrepHit {
                        path: abs.clone(),
                        rel: rel_clone.clone(),
                        line: line_num,
                        text: line_text,
                    });
                    Ok(true)
                }),
            );

            WalkState::Continue
        })
    });

    let final_hits = Arc::try_unwrap(hits)
        .map(|m| m.into_inner().unwrap())
        .unwrap_or_default();

    GrepResponse {
        hits: final_hits,
        truncated: truncated.load(Ordering::Relaxed),
        files_scanned: scanned.load(Ordering::Relaxed),
        cancelled: cancel(),
    }
}

/// What a content search matches.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GrepPattern {
    /// A regular expression, case-sensitive unless asked otherwise.
    Regex {
        pattern: String,
        case_insensitive: bool,
    },
    /// Literal text with smart case: all-lowercase matches any case.
    Literal { text: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrepRequest {
    /// Workspace path of the directory to search.
    pub root: String,
    pub pattern: GrepPattern,
    /// Only files whose root-relative path matches one of these.
    pub globs: Vec<String>,
    pub max_results: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlobRequest {
    pub root: String,
    pub pattern: String,
    pub max_results: Option<usize>,
}

/// Runs a content search over `root_path` (a host path) whose workspace
/// spelling is `request.root`.
pub fn grep(
    root_path: &Path,
    workspace: &WorkspaceEnv,
    request: &GrepRequest,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<GrepResponse, String> {
    let matcher = match &request.pattern {
        GrepPattern::Regex { pattern, case_insensitive } => {
            if pattern.is_empty() {
                return Err("empty pattern".into());
            }
            not_a_dir(root_path, &request.root)?;
            RegexMatcherBuilder::new()
                .case_insensitive(*case_insensitive)
                .line_terminator(Some(b'\n'))
                .build(pattern)
                .map_err(|e| format!("bad regex: {e}"))?
        }
        GrepPattern::Literal { text } => {
            if text.trim().is_empty() {
                return Err("empty pattern".into());
            }
            not_a_dir(root_path, &request.root)?;
            RegexMatcherBuilder::new()
                .case_smart(true)
                .line_terminator(Some(b'\n'))
                .build(&escape_literal(text))
                .map_err(|e| format!("bad pattern: {e}"))?
        }
    };
    let cap = request
        .max_results
        .unwrap_or(DEFAULT_MAX_RESULTS)
        .clamp(1, HARD_MAX_RESULTS);
    let globs = build_globset(&request.globs)?;
    Ok(search_tree(
        root_path,
        &request.root,
        workspace,
        &matcher,
        &globs,
        cap,
        cancel,
    ))
}

fn not_a_dir(root_path: &Path, root: &str) -> Result<(), String> {
    if root_path.is_dir() {
        Ok(())
    } else {
        Err(format!("not a directory: {root}"))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlobHit {
    pub path: String,
    pub rel: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlobResponse {
    pub hits: Vec<GlobHit>,
    pub truncated: bool,
}

/// Lists files under `root_path` whose root-relative path matches the glob.
pub fn glob(
    root_path: &Path,
    workspace: &WorkspaceEnv,
    request: &GlobRequest,
) -> Result<GlobResponse, String> {
    if request.pattern.is_empty() {
        return Err("empty pattern".into());
    }
    not_a_dir(root_path, &request.root)?;
    let root_display = request.root.as_str();
    let pattern = &request.pattern;
    let max_results = request.max_results;
    let cap = max_results.unwrap_or(500).clamp(1, HARD_MAX_RESULTS);

    let glob = Glob::new(pattern).map_err(|e| format!("bad glob: {e}"))?;
    let mut gb = GlobSetBuilder::new();
    gb.add(glob);
    let set = gb.build().map_err(|e| format!("globset build: {e}"))?;

    let walker = WalkBuilder::new(root_path)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .follow_links(false)
        .build();

    let mut hits: Vec<GlobHit> = Vec::new();
    let mut truncated = false;
    for dent in walker.flatten() {
        if hits.len() >= cap {
            truncated = true;
            break;
        }
        if !dent.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = dent.path();
        let rel = match path.strip_prefix(root_path) {
            Ok(r) => to_canon(r),
            Err(_) => continue,
        };
        if !set.is_match(&rel) {
            continue;
        }
        hits.push(GlobHit {
            path: display_path(path, root_path, root_display, workspace),
            rel,
        });
    }

    Ok(GlobResponse { hits, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_literal_escapes_regex_meta() {
        assert_eq!(escape_literal("a.b(c)"), "a\\.b\\(c\\)");
        assert_eq!(escape_literal("plain text"), "plain text");
    }

    #[test]
    fn search_tree_respects_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\nfind me here\n").unwrap();
        let matcher = RegexMatcherBuilder::new().build("find").unwrap();
        let ws = WorkspaceEnv::from_option(None);
        let root_display = dir.path().to_string_lossy().to_string();

        let live = search_tree(
            dir.path(),
            &root_display,
            &ws,
            &matcher,
            &None,
            100,
            &|| false,
        );
        assert_eq!(live.hits.len(), 1, "uncancelled search finds the match");

        let stopped = search_tree(
            dir.path(),
            &root_display,
            &ws,
            &matcher,
            &None,
            100,
            &|| true,
        );
        assert!(stopped.hits.is_empty(), "cancelled search yields nothing");
    }

    #[test]
    fn a_walk_stops_soon_after_it_is_superseded_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..400 {
            std::fs::write(dir.path().join(format!("f{i}.txt")), "needle\n").unwrap();
        }
        let matcher = RegexMatcherBuilder::new().build("needle").unwrap();
        let ws = WorkspaceEnv::from_option(None);
        let root_display = dir.path().to_string_lossy().to_string();
        let polls = AtomicUsize::new(0);
        let cancel = || polls.fetch_add(1, Ordering::SeqCst) >= 20;

        let out = search_tree(
            dir.path(),
            &root_display,
            &ws,
            &matcher,
            &None,
            2000,
            &cancel,
        );
        assert!(out.cancelled);
        assert!(
            out.files_scanned < 100,
            "kept scanning after cancellation: {}",
            out.files_scanned
        );
    }

}
