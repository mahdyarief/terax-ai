use tauri::State;

use super::blocking;
use super::supersede::Supersede;
use crate::modules::workspace::{backend_for, WorkspaceEnv};
pub use terax_workspace_core::grep::{GlobHit, GlobResponse, GrepHit, GrepResponse};
use terax_workspace_core::grep::{GlobRequest, GrepPattern, GrepRequest};

/// Supersession key for the command palette's live content search: each new
/// query cancels the previous walk on its worker threads.
const INTERACTIVE_KEY: &str = "grep-interactive";

#[tauri::command]
pub async fn fs_grep(
    pattern: String,
    root: String,
    glob: Option<Vec<String>>,
    case_insensitive: Option<bool>,
    max_results: Option<usize>,
    workspace: Option<WorkspaceEnv>,
) -> Result<GrepResponse, String> {
    blocking(move || {
        run_grep(
            pattern,
            root,
            glob,
            case_insensitive,
            max_results,
            workspace,
        )
    })
    .await
}

pub fn run_grep(
    pattern: String,
    root: String,
    glob: Option<Vec<String>>,
    case_insensitive: Option<bool>,
    max_results: Option<usize>,
    workspace: Option<WorkspaceEnv>,
) -> Result<GrepResponse, String> {
    let request = GrepRequest {
        root,
        pattern: GrepPattern::Regex {
            pattern,
            case_insensitive: case_insensitive.unwrap_or(false),
        },
        globs: glob.unwrap_or_default(),
        max_results,
    };
    backend_for(&WorkspaceEnv::from_option(workspace)).grep(&request, &|| false)
}

/// Interactive content search for the command palette. Treats the query as a
/// literal (smart-case), and self-cancels when a newer query arrives. The
/// generation is bumped when the command is dispatched, so the previous walk
/// stops even though it is still occupying a blocking thread.
#[tauri::command]
pub async fn fs_grep_interactive(
    supersede: State<'_, Supersede>,
    pattern: String,
    root: String,
    max_results: Option<usize>,
    workspace: Option<WorkspaceEnv>,
) -> Result<GrepResponse, String> {
    let ticket = supersede.begin(INTERACTIVE_KEY);
    blocking(move || {
        run_grep_interactive(pattern, root, max_results, workspace, &|| {
            ticket.is_superseded()
        })
    })
    .await
}

pub fn run_grep_interactive(
    pattern: String,
    root: String,
    max_results: Option<usize>,
    workspace: Option<WorkspaceEnv>,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<GrepResponse, String> {
    let request = GrepRequest {
        root,
        pattern: GrepPattern::Literal { text: pattern },
        globs: Vec::new(),
        max_results,
    };
    backend_for(&WorkspaceEnv::from_option(workspace)).grep(&request, cancel)
}

#[tauri::command]
pub async fn fs_glob(
    pattern: String,
    root: String,
    max_results: Option<usize>,
    workspace: Option<WorkspaceEnv>,
) -> Result<GlobResponse, String> {
    blocking(move || run_glob(pattern, root, max_results, workspace)).await
}

pub fn run_glob(
    pattern: String,
    root: String,
    max_results: Option<usize>,
    workspace: Option<WorkspaceEnv>,
) -> Result<GlobResponse, String> {
    let request = GlobRequest {
        root,
        pattern,
        max_results,
    };
    backend_for(&WorkspaceEnv::from_option(workspace)).glob(&request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_search_reports_not_cancelled_when_it_runs_to_completion() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "Find Me\n").unwrap();
        let out = run_grep_interactive(
            "find me".into(),
            dir.path().to_string_lossy().into_owned(),
            None,
            None,
            &|| false,
        )
        .unwrap();
        assert!(!out.cancelled);
        assert_eq!(
            out.hits.len(),
            1,
            "smart-case: lowercase query matches any case"
        );
    }
}
