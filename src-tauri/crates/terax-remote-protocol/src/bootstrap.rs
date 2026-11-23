//! What runs on the server before the agent: one `sh` command that reports
//! the machine and any installed agent, takes one instruction (`run`, or
//! `install` followed by the binary), verifies and installs, and then `exec`s
//! the agent, so the same SSH session carries the protocol.
//!
//! The exchange, on the session's stdout and stdin:
//!
//! ```text
//! <- TERAX-BOOTSTRAP 1
//! <- os=Linux
//! <- arch=x86_64
//! <- hasher=sha256sum          (or none)
//! <- sha256=<hex>              (of the installed agent, or none)
//! <- END
//! -> run                       or: install <bytes> <sha256>, then the bytes
//! <- READY                     (or ERROR <reason>, and the session ends)
//!    ... the agent's protocol from here on
//! ```
//!
//! The client sends nothing after its instruction until it has read `READY`,
//! so nothing the script reads can belong to the agent.

/// Raised when the exchange above changes.
pub const BOOTSTRAP_PROTOCOL: &str = "TERAX-BOOTSTRAP 1";

/// Where agents live on the server, under the user's home.
pub const INSTALL_DIR: &str = ".terax-server";

/// The line that runs the installed agent.
pub const RUN: &str = "run\n";

/// The agents this app can install, by the server's `uname -m`.
const PLATFORMS: [(&str, &str); 4] = [
    ("x86_64", "x86_64-linux-musl"),
    ("amd64", "x86_64-linux-musl"),
    ("aarch64", "aarch64-linux-musl"),
    ("arm64", "aarch64-linux-musl"),
];

/// The command for `ssh <host> <command>`. The user's login shell parses it
/// (bash, zsh, fish, csh...), so the script is one line inside single quotes
/// with no quote, backslash or newline of its own, which every one of them
/// passes to `sh` unchanged.
pub fn remote_command(version: &str) -> Result<String, String> {
    check_version(version)?;
    let script = script(version);
    debug_assert!(!script.contains(['\'', '\\', '\n', '\r']));
    Ok(format!("sh -c '{script}'"))
}

/// The instruction that uploads an agent of `len` bytes; the bytes follow it.
pub fn install_line(len: u64, sha256: &str) -> String {
    format!("install {len} {sha256}\n")
}

fn check_version(version: &str) -> Result<(), String> {
    let ok = !version.is_empty()
        && version.len() <= 64
        && !version.starts_with('.')
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'));
    if ok {
        Ok(())
    } else {
        Err(format!("unusable agent version `{version}`"))
    }
}

fn script(version: &str) -> String {
    [
        "umask 077",
        &format!("v={version}"),
        &format!("d=\"$HOME/{INSTALL_DIR}\""),
        "a=\"$d/$v/terax-agent\"",
        "mkdir -p \"$d/$v\" || { echo \"ERROR cannot create $d/$v\"; exit 1; }",
        "chmod 700 \"$d\" \"$d/$v\"",
        // An upload cut off by a dropped session leaves its temp file; one
        // older than a day belongs to no upload still running.
        "find \"$d/$v\" -maxdepth 1 -name \".terax-agent.*\" -mtime +0 -exec rm -f {} + 2>/dev/null",
        "h=none",
        "if command -v sha256sum >/dev/null 2>&1; then h=sha256sum; fi",
        "s=none",
        "if [ \"$h\" != none ] && [ -f \"$a\" ]; then s=$(sha256sum \"$a\" | cut -d\" \" -f1); fi",
        &format!("echo \"{BOOTSTRAP_PROTOCOL}\""),
        "echo \"os=$(uname -s)\"",
        "echo \"arch=$(uname -m)\"",
        "echo \"hasher=$h\"",
        "echo \"sha256=$s\"",
        "echo END",
        "read -r c n x",
        // `install <bytes> <sha256>`: read exactly that many bytes, verify,
        // and move into place; a short or altered upload leaves nothing.
        "if [ \"$c\" = install ]; then \
         case \"$n\" in \"\"|*[!0-9]*) echo \"ERROR bad install size\"; exit 1;; esac; \
         t=\"$d/$v/.terax-agent.$$\"; \
         if head -c 0 </dev/null >/dev/null 2>&1; then head -c \"$n\" >\"$t\"; \
         else dd bs=1 count=\"$n\" of=\"$t\" 2>/dev/null; fi; \
         g=$(sha256sum \"$t\" | cut -d\" \" -f1); \
         if [ \"$g\" != \"$x\" ]; then rm -f \"$t\"; echo \"ERROR checksum mismatch\"; exit 1; fi; \
         chmod 700 \"$t\" && mv -f \"$t\" \"$a\" || { rm -f \"$t\"; echo \"ERROR cannot install\"; exit 1; }; \
         elif [ \"$c\" != run ]; then exit 0; fi",
        // Mark this version used, and drop versions untouched for 30 days.
        "touch \"$d/$v\"",
        "find \"$d\" -mindepth 1 -maxdepth 1 -type d ! -name \"$v\" -mtime +30 -exec rm -rf {} + 2>/dev/null",
        "echo READY",
        "exec \"$a\"",
    ]
    .join("; ")
}

/// What the script reported about the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub os: String,
    pub arch: String,
    /// `None` when the server has no `sha256sum`.
    pub hasher: Option<String>,
    /// The installed agent's SHA-256, if there is one.
    pub sha256: Option<String>,
}

/// Parses the lines from the protocol line to `END` (both included).
pub fn parse_report(lines: &[&str]) -> Result<Report, String> {
    let mut lines = lines.iter().map(|line| line.trim_end_matches('\r'));
    match lines.next() {
        Some(BOOTSTRAP_PROTOCOL) => {}
        Some(other) => return Err(format!("unexpected bootstrap output: `{other}`")),
        None => return Err("no bootstrap output".into()),
    }
    let (mut os, mut arch, mut hasher, mut sha256): (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = (None, None, None, None);
    for line in lines {
        if line == "END" {
            let missing = |name: &str| format!("the bootstrap did not report {name}");
            let optional = |value: String| Some(value).filter(|v| v != "none");
            return Ok(Report {
                os: os.ok_or_else(|| missing("os"))?,
                arch: arch.ok_or_else(|| missing("arch"))?,
                hasher: optional(hasher.ok_or_else(|| missing("hasher"))?),
                sha256: optional(sha256.ok_or_else(|| missing("sha256"))?),
            });
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("unexpected bootstrap line: `{line}`"));
        };
        let slot = match key {
            "os" => &mut os,
            "arch" => &mut arch,
            "hasher" => &mut hasher,
            "sha256" => &mut sha256,
            _ => return Err(format!("unexpected bootstrap key: `{key}`")),
        };
        *slot = Some(value.to_string());
    }
    Err("the bootstrap output ended before END".into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// The installed agent is this app's: run it.
    Run,
    /// Upload this app's agent for `platform`, then run it.
    Install { platform: &'static str },
    /// This server cannot run an agent from this app; why, for the user.
    Unsupported(String),
}

/// What to do on a server, given the agents this app carries:
/// `bundled(platform)` is the SHA-256 of the agent for `platform`, if any.
pub fn plan(report: &Report, bundled: impl Fn(&str) -> Option<String>) -> Plan {
    if report.os != "Linux" {
        return Plan::Unsupported(format!(
            "{} servers are not supported; Terax runs on Linux servers",
            report.os
        ));
    }
    let Some(platform) = PLATFORMS
        .iter()
        .find(|(arch, _)| *arch == report.arch)
        .map(|(_, platform)| *platform)
    else {
        return Plan::Unsupported(format!(
            "{} servers are not supported; Terax runs on x86_64 and aarch64",
            report.arch
        ));
    };
    if report.hasher.is_none() {
        return Plan::Unsupported(
            "the server has no `sha256sum`, so the agent cannot be verified".into(),
        );
    }
    let Some(expected) = bundled(platform) else {
        return Plan::Unsupported(format!(
            "this build of Terax carries no agent for {platform}"
        ));
    };
    if report.sha256.as_deref() == Some(expected.as_str()) {
        Plan::Run
    } else {
        Plan::Install { platform }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(arch: &str, sha256: Option<&str>) -> Report {
        Report {
            os: "Linux".into(),
            arch: arch.into(),
            hasher: Some("sha256sum".into()),
            sha256: sha256.map(Into::into),
        }
    }

    fn both(platform: &str) -> Option<String> {
        Some(format!("sha-of-{platform}"))
    }

    #[test]
    fn the_command_survives_any_login_shell() {
        let command = remote_command("0.9.4-dy").unwrap();
        let script = command
            .strip_prefix("sh -c '")
            .and_then(|rest| rest.strip_suffix('\''))
            .unwrap();
        assert!(!script.contains(['\'', '\\', '\n', '\r']), "{script}");
        assert!(script.contains("v=0.9.4-dy;"));
        assert!(script.is_ascii());
    }

    #[test]
    fn versions_that_could_escape_the_script_are_refused() {
        for bad in [
            "",
            "1'; rm -rf ~; '",
            "1 2",
            "../x",
            ".hidden",
            "a$b",
            "a/b",
            &"9".repeat(65),
        ] {
            assert!(remote_command(bad).is_err(), "{bad:?}");
        }
        assert!(remote_command("0.9.5-dy+build.7").is_ok());
    }

    #[test]
    fn reports_parse_with_or_without_an_installed_agent() {
        let lines = [
            BOOTSTRAP_PROTOCOL,
            "os=Linux",
            "arch=x86_64",
            "hasher=sha256sum",
            "sha256=abc",
            "END",
        ];
        assert_eq!(parse_report(&lines).unwrap(), report("x86_64", Some("abc")));
        let none = [
            BOOTSTRAP_PROTOCOL,
            "os=Linux",
            "arch=aarch64",
            "hasher=none",
            "sha256=none\r",
            "END",
        ];
        let parsed = parse_report(&none).unwrap();
        assert_eq!((parsed.hasher, parsed.sha256), (None, None));
    }

    #[test]
    fn unexpected_output_is_an_error() {
        for lines in [
            &[][..],
            &["Welcome to the server!", "END"][..],
            &[BOOTSTRAP_PROTOCOL, "os=Linux"][..],
            &[BOOTSTRAP_PROTOCOL, "os=Linux", "END"][..],
            &[BOOTSTRAP_PROTOCOL, "colour=blue", "END"][..],
            &[BOOTSTRAP_PROTOCOL, "garbage", "END"][..],
        ] {
            assert!(parse_report(lines).is_err(), "{lines:?}");
        }
    }

    #[test]
    fn the_plan_follows_the_server_and_the_installed_agent() {
        assert_eq!(
            plan(&report("x86_64", None), both),
            Plan::Install {
                platform: "x86_64-linux-musl"
            }
        );
        assert_eq!(
            plan(&report("arm64", Some("old")), both),
            Plan::Install {
                platform: "aarch64-linux-musl"
            }
        );
        assert_eq!(
            plan(&report("aarch64", Some("sha-of-aarch64-linux-musl")), both),
            Plan::Run
        );
    }

    #[test]
    fn servers_the_app_cannot_serve_are_named() {
        let unsupported = |plan: Plan, needle: &str| match plan {
            Plan::Unsupported(why) => assert!(why.contains(needle), "{why}"),
            other => panic!("{other:?}"),
        };
        let mut mac = report("arm64", None);
        mac.os = "Darwin".into();
        unsupported(plan(&mac, both), "Darwin");
        unsupported(plan(&report("riscv64", None), both), "riscv64");
        let mut bare = report("x86_64", None);
        bare.hasher = None;
        unsupported(plan(&bare, both), "sha256sum");
        unsupported(plan(&report("x86_64", None), |_| None), "x86_64-linux-musl");
    }

    #[test]
    fn the_install_line_carries_size_and_hash() {
        assert_eq!(install_line(2_863_320, "ab12"), "install 2863320 ab12\n");
    }
}
