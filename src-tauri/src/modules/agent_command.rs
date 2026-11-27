//! Whether a CLI agent's start command can be found before it is typed into a
//! terminal. Only a hint: a shell alias or function is invisible to a PATH
//! lookup on this machine, so "not found" never blocks a launch.

/// The program a start command runs, or `None` when that cannot be told
/// without a shell (variables, subshells, globs, `~`, relative paths).
pub(crate) fn command_program(command: &str) -> Option<String> {
    let mut words = command.split_whitespace();
    loop {
        let word = words.next()?;
        // `FOO=1 claude`: leading variable assignments.
        if let Some(eq) = word.find('=') {
            let name = &word[..eq];
            if eq > 0 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
        }
        if matches!(word, "env" | "exec" | "command") {
            continue;
        }
        let word = word.trim_matches(|c| c == '"' || c == '\'');
        if word.is_empty()
            || word.starts_with('-')
            || word.contains(|c: char| "$`()[]{};|&<>*?~!\"'".contains(c))
        {
            return None;
        }
        let has_separator = word.contains('/') || word.contains('\\');
        if has_separator && !std::path::Path::new(word).is_absolute() && !word.starts_with('/') {
            return None;
        }
        return Some(word.to_string());
    }
}

fn available_here(program: &str) -> Option<bool> {
    Some(crate::modules::lsp::resolve_binary(program).is_some())
}

#[cfg(windows)]
fn available_in_wsl(distro: &str, program: &str) -> Option<bool> {
    use crate::modules::workspace::{normalize_wsl_value, wsl_exec_capture, wsl_login_shell};
    // The user's shell, interactive, so PATH from `.bashrc` (nvm, ~/.local/bin)
    // and aliases count. fish has its own syntax; plain sh is the fallback.
    let shell = wsl_login_shell(distro.to_string()).unwrap_or_else(|_| "sh".into());
    let shell = if shell.ends_with("fish") {
        "sh".to_string()
    } else {
        shell
    };
    const SCRIPT: &str = r#"if command -v -- "$1" >/dev/null 2>&1; then echo terax-found; else echo terax-missing; fi"#;
    let out = wsl_exec_capture(distro, &shell, &["-lic", SCRIPT, "terax", program]).ok()?;
    match normalize_wsl_value(out, "").as_str() {
        "terax-found" => Some(true),
        "terax-missing" => Some(false),
        _ => None,
    }
}

#[cfg(not(windows))]
fn available_in_wsl(_distro: &str, _program: &str) -> Option<bool> {
    None
}

/// `Some(found)` for this machine or a WSL distro; `None` when it cannot be
/// told. A server over SSH is never asked: its PATH is the server's.
#[tauri::command]
pub async fn agent_command_available(
    command: String,
    distro: Option<String>,
) -> Result<Option<bool>, String> {
    let Some(program) = command_program(&command) else {
        return Ok(None);
    };
    super::fs::blocking(move || {
        Ok(match distro.as_deref().filter(|d| !d.is_empty()) {
            Some(distro) => available_in_wsl(distro, &program),
            None => available_here(&program),
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::{available_here, command_program};

    #[test]
    fn a_program_on_path_is_found_and_a_made_up_one_is_not() {
        // The test runs under cargo, so cargo is on PATH.
        assert_eq!(available_here("cargo"), Some(true));
        assert_eq!(available_here("terax-no-such-agent-cli"), Some(false));
    }

    #[test]
    fn the_program_is_the_first_plain_word() {
        assert_eq!(command_program("claude").as_deref(), Some("claude"));
        assert_eq!(command_program("  ccr code --x ").as_deref(), Some("ccr"));
        assert_eq!(command_program("FOO=1 BAR_2=x agy").as_deref(), Some("agy"));
        assert_eq!(command_program("env exec codex").as_deref(), Some("codex"));
        assert_eq!(command_program("\"opencode\"").as_deref(), Some("opencode"));
        assert_eq!(
            command_program("/usr/bin/pi").as_deref(),
            Some("/usr/bin/pi")
        );
    }

    #[test]
    fn shell_syntax_cannot_be_told() {
        for command in [
            "",
            "$HOME/bin/claude",
            "~/bin/claude",
            "./claude",
            "(cd x && claude)",
            "claude*",
            "-x",
        ] {
            assert_eq!(command_program(command), None, "{command}");
        }
    }
}
