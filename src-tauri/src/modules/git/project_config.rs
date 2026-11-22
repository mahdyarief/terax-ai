use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::errors::{GitError, Result};
use super::plumbing::open_worktree;
use super::{refs, worktree};
use super::utils::ResolvedGitDirectory;
use crate::modules::workspace::{within, WorkspaceEnv, WorkspaceRegistry};
use terax_workspace_core::paths::join;
use terax_workspace_core::workspace::{EntryKind, ReadRequest};

const CONFIG_LIMIT: u64 = 256 * 1024;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub worktree: WorktreeConfig,
    pub scripts: Scripts,
    pub ports: Ports,
    pub layout: Layout,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct WorktreeConfig {
    pub dir: String,
    pub include: Vec<String>,
    pub shared: Vec<String>,
    pub branch_prefix: String,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            dir: "~/.terax/worktrees/{project}/{task}".into(),
            include: vec![".env".into(), ".env.local".into()],
            shared: vec!["node_modules/.cache".into()],
            branch_prefix: "terax/".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct Scripts {
    pub setup: String,
    pub run: String,
    pub archive: String,
    pub run_mode: RunMode,
}

impl Default for Scripts {
    fn default() -> Self {
        Self {
            setup: "pnpm install".into(),
            run: "pnpm dev --port $TERAX_PORT".into(),
            archive: "docker compose down".into(),
            run_mode: RunMode::Concurrent,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    Concurrent,
    Exclusive,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct Ports {
    pub base: u16,
    pub per_task: u16,
}

impl Default for Ports {
    fn default() -> Self {
        Self {
            base: 3000,
            per_task: 10,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Layout {
    pub default: CardLayout,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CardLayout {
    pub cards: Vec<String>,
}

impl Default for CardLayout {
    fn default() -> Self {
        Self {
            cards: vec!["agent".into(), "review | preview".into()],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Action {
    pub name: String,
    pub run: String,
}

fn invalid(text: &str, key: &str, why: impl std::fmt::Display) -> GitError {
    let line = text
        .lines()
        .position(|line| {
            line.trim_start()
                .strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        })
        .map_or(1, |n| n + 1);
    GitError::command(".terax/project.toml", format!("line {line}: {key}: {why}"))
}

pub fn parse(text: &str) -> Result<ProjectConfig> {
    let config: ProjectConfig = toml::from_str(text)
        .map_err(|e| GitError::command(".terax/project.toml", e.to_string()))?;
    validate_template(&config.worktree.dir).map_err(|e| invalid(text, "dir", e))?;
    if !refs::is_plausible_ref_name(&format!("{}task", config.worktree.branch_prefix)) {
        return Err(invalid(text, "branch_prefix", "invalid branch prefix"));
    }
    for (key, entries) in [
        ("include", &config.worktree.include),
        ("shared", &config.worktree.shared),
    ] {
        if entries.len() > 64 {
            return Err(invalid(text, key, "at most 64 entries"));
        }
        for entry in entries {
            validate_entry(entry).map_err(|e| invalid(text, key, e))?;
            if key == "shared" && entry.contains(['*', '?', '[', '{']) {
                return Err(invalid(text, key, "shared entries must be literal paths"));
            }
        }
    }
    if config.ports.base == 0
        || config.ports.per_task < 10
        || u32::from(config.ports.base) + u32::from(config.ports.per_task) > 65536
    {
        return Err(invalid(
            text,
            "per_task",
            "ports require base >= 1, per_task >= 10 and a block within 1..65535",
        ));
    }
    Ok(config)
}

fn validate_entry(entry: &str) -> std::result::Result<(), String> {
    worktree::validate_include_pattern(entry)?;
    if entry.contains(':') || entry.split('/').any(|part| part.ends_with(['.', ' '])) {
        return Err("alternate streams and trailing dots/spaces are not allowed".into());
    }
    if entry.contains(['*', '?', '[', '{']) {
        globset::GlobBuilder::new(entry)
            .literal_separator(true)
            .build()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn validate_template(template: &str) -> std::result::Result<(), &'static str> {
    if template.is_empty() || template.len() > 4096 || template.chars().any(char::is_control) {
        return Err("empty, oversized or invalid directory template");
    }
    let normalized = template.replace('\\', "/");
    let parts: Vec<_> = normalized.split('/').collect();
    let without_drive = if normalized.len() >= 3
        && normalized.as_bytes()[0].is_ascii_alphabetic()
        && normalized.as_bytes()[1] == b':'
        && normalized.as_bytes()[2] == b'/'
    {
        &normalized[3..]
    } else {
        &normalized
    };
    if without_drive.contains([':', '*', '?', '<', '>', '|', '"'])
        || parts.iter().any(|p| p.ends_with(['.', ' ']))
    {
        return Err("invalid directory path component");
    }

    if parts
        .iter()
        .any(|part| *part == ".." || *part == "." || part.eq_ignore_ascii_case(".git"))
    {
        return Err("directory traversal and git internals are not allowed");
    }
    if normalized.matches("{task}").count() != 1
        || !parts.last().is_some_and(|part| part.contains("{task}"))
    {
        return Err("{task} must appear once, in the final path component");
    }
    if normalized
        .replace("{task}", "task")
        .replace("{project}", "project")
        .contains(['{', '}'])
    {
        return Err("unknown directory template placeholder");
    }
    if normalized.contains('~') && (!normalized.starts_with("~/") || normalized[2..].contains('~'))
    {
        return Err("~ is supported only at the beginning as ~/");
    }
    Ok(())
}

/// A small text file under the repository, refused when it resolves outside
/// it or is not a regular file. `None` when it does not exist.
pub(crate) fn read_confined(repo: &ResolvedGitDirectory, relative: &str) -> Result<Option<String>> {
    let ws = repo.backend();
    let resolved = match ws.canonicalize(&join(&repo.ws_path, relative)) {
        Ok(path) => path,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !within(ws.env(), &resolved, &repo.ws_path) {
        return Err(GitError::PathOutsideWorkspace(PathBuf::from(resolved)));
    }
    if ws.metadata(&resolved)?.kind != EntryKind::File {
        return Err(GitError::InvalidPath(format!(
            "{relative} must be a regular file"
        )));
    }
    let read = ws.read(
        &resolved,
        ReadRequest {
            offset: 0,
            limit: CONFIG_LIMIT,
        },
    )?;
    if read.bytes.len() as u64 > CONFIG_LIMIT {
        return Err(GitError::InvalidPath(format!("{relative} exceeds 256 KiB")));
    }
    let text = String::from_utf8(read.bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(text))
}

pub fn read(
    registry: &WorkspaceRegistry,
    root: &str,
    workspace: &WorkspaceEnv,
) -> Result<ProjectConfig> {
    let repo = open_worktree(registry, root, workspace)?;
    if workspace.is_wsl() {
        return Err(GitError::Unsupported(
            "task configuration is not supported in WSL workspaces yet",
        ));
    }
    let mut config =
        parse(&read_confined(&repo, ".terax/project.toml")?.unwrap_or_default())?;
    if let Some(text) = read_confined(&repo, ".worktreeinclude")? {
        for (line, entry) in text.lines().enumerate() {
            let entry = entry.trim();
            if entry.is_empty() || entry.starts_with('#') {
                continue;
            }
            validate_entry(entry).map_err(|why| {
                GitError::command(".worktreeinclude", format!("line {}: {why}", line + 1))
            })?;
            if !config.worktree.include.iter().any(|p| p == entry) {
                config.worktree.include.push(entry.into());
            }
        }
    }
    if config.worktree.include.len() > 64 {
        return Err(GitError::InvalidPath(
            "at most 64 combined include patterns".into(),
        ));
    }
    Ok(config)
}

pub(crate) fn location(
    config: &WorktreeConfig,
    repo: &Path,
    home: &Path,
    task: &str,
) -> Result<PathBuf> {
    validate_template(&config.dir).map_err(|e| GitError::InvalidPath(e.into()))?;
    let project = refs::slugify(&repo.file_name().unwrap_or_default().to_string_lossy(), 40);
    let template = config.dir.replace('\\', "/");
    let template = if let Some(tail) = template.strip_prefix("~/") {
        home.join(tail)
    } else {
        let path = PathBuf::from(&template);
        if path.is_absolute() {
            path
        } else {
            repo.join(path)
        }
    };
    let expanded = PathBuf::from(
        template
            .to_string_lossy()
            .replace("{project}", &project)
            .replace("{task}", task),
    );
    validate_location(&expanded, repo, home)?;
    // Resolve the static root first; placeholder directories may not redirect it.
    let mut root = PathBuf::new();
    for component in template.components() {
        if component.as_os_str().to_string_lossy().contains('{') {
            break;
        }
        root.push(component);
    }
    std::fs::create_dir_all(&root)?;
    let canonical_root = std::fs::canonicalize(&root)?;
    let tail = expanded
        .strip_prefix(&root)
        .map_err(|_| GitError::InvalidPath(config.dir.clone()))?;
    let relative = super::plumbing::forward_slashes(&tail.to_string_lossy());
    let parent = worktree::create_dirs_confined(&canonical_root, &relative)
        .map_err(|why| GitError::InvalidPath(why.into()))?;
    let target = parent.join(
        expanded
            .file_name()
            .ok_or_else(|| GitError::InvalidPath(config.dir.clone()))?,
    );
    validate_location(&target, repo, home)?;
    Ok(target)
}

fn resolved_destination(path: &Path) -> std::io::Result<PathBuf> {
    let mut tail = Vec::new();
    let mut cursor = path;
    loop {
        match std::fs::symlink_metadata(cursor) {
            Ok(_) => {
                let mut resolved = std::fs::canonicalize(cursor)?;
                for name in tail.iter().rev() {
                    resolved.push(name);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tail.push(cursor.file_name().ok_or(error)?);
                cursor = cursor.parent().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent directory")
                })?;
            }
            Err(error) => return Err(error),
        }
    }
}

pub(crate) fn validate_location(path: &Path, repo: &Path, home: &Path) -> Result<()> {
    let target = resolved_destination(path)?;
    let repo = std::fs::canonicalize(repo)?;
    let default_root = std::fs::canonicalize(home)?.join(".terax/worktrees");
    let siblings = repo
        .parent()
        .ok_or_else(|| GitError::InvalidPath("repository has no parent".into()))?;
    let key = |path: &Path| {
        let plain = crate::modules::fs::to_canon(path);
        PathBuf::from(if cfg!(windows) {
            plain.to_lowercase()
        } else {
            plain
        })
    };
    let target_key = key(&target);
    if ![&default_root, siblings].iter().any(|root| {
        let root = key(root);
        target_key != root && target_key.starts_with(root)
    }) {
        return Err(GitError::InvalidPath(format!(
            "project.toml worktree dir resolves to {}; it must be under {} or {}",
            crate::modules::fs::to_canon(&target),
            crate::modules::fs::to_canon(&default_root),
            crate::modules::fs::to_canon(siblings),
        )));
    }
    let repo_key = key(&repo);
    if target_key.starts_with(&repo_key) || repo_key.starts_with(&target_key) {
        return Err(GitError::InvalidPath(format!(
            "task directory {} overlaps the repository",
            crate::modules::fs::to_canon(&target)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::git::utils::canonical_dir;

    /// `.terax` linked out of the repository: the file's text stays under the
    /// repository, but it resolves outside, and is refused.
    #[test]
    fn a_config_file_reached_through_a_link_out_of_the_repository_is_refused() {
        let repo_dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("project.toml"), "").unwrap();
        let link = repo_dir.path().join(".terax");
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        #[cfg(windows)]
        {
            // A junction needs no symlink privilege.
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(outside.path())
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "mklink /J failed");
        }
        let registry = WorkspaceRegistry::default();
        registry.authorize(repo_dir.path()).unwrap();
        let repo = canonical_dir(
            &registry,
            &repo_dir.path().to_string_lossy(),
            &WorkspaceEnv::Local,
        )
        .unwrap();

        let err = read_confined(&repo, ".terax/project.toml").unwrap_err();
        assert!(matches!(err, GitError::PathOutsideWorkspace(_)), "{err:?}");
        assert!(read_confined(&repo, ".worktreeinclude").unwrap().is_none());
    }
}
