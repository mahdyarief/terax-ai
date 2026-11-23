//! The host names a user can pick, from `~/.ssh/config`.
//!
//! Only literal `Host` names: patterns (`*`, `?`) and negations (`!`) match
//! many hosts and name none. `Include` is followed one level (as far as the
//! picker goes; ssh itself still reads the whole config when it connects).
//! Nothing here decides how ssh connects; the user's ssh does.

use std::fs;
use std::path::{Path, PathBuf};

/// One file's `Host` names, in order, and the `Include` arguments it names.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub hosts: Vec<String>,
    pub includes: Vec<String>,
}

/// Splits a config line into its keyword and its arguments: `Keyword args`,
/// `Keyword=args`, comments and blank lines ignored, quoted arguments kept
/// whole.
fn split(line: &str) -> Option<(String, Vec<String>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let keyword = line[..end].to_ascii_lowercase();
    let rest = line[end..].trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest);
    let mut args = Vec::new();
    let mut chars = rest.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let Some(&first) = chars.peek() else { break };
        if first == '#' {
            break;
        }
        let mut arg = String::new();
        if first == '"' {
            chars.next();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                arg.push(c);
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                arg.push(c);
                chars.next();
            }
        }
        args.push(arg);
    }
    Some((keyword, args))
}

/// Whether `name` names one host a user could connect to.
fn literal(name: &str) -> bool {
    !name.is_empty() && !name.contains(['*', '?', '!']) && valid(name)
}

/// Host names must also pass the transport's own check, or picking one
/// could only fail.
fn valid(name: &str) -> bool {
    terax_workspace_core::env::SshHost::try_from(name).is_ok()
}

pub fn parse(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    for line in text.lines() {
        let Some((keyword, args)) = split(line) else {
            continue;
        };
        match keyword.as_str() {
            "host" => {
                for name in args {
                    if literal(&name) && !parsed.hosts.contains(&name) {
                        parsed.hosts.push(name);
                    }
                }
            }
            "include" => parsed.includes.extend(args),
            _ => {}
        }
    }
    parsed
}

/// The files an `Include` argument names: `~/` is the home directory, a
/// relative path is under `~/.ssh`, and `*`/`?` in the last component match
/// file names there (sorted, as ssh does).
pub fn include_paths(argument: &str, home: &Path) -> Vec<PathBuf> {
    let path = if let Some(rest) = argument.strip_prefix("~/") {
        home.join(rest)
    } else if Path::new(argument).is_absolute() {
        PathBuf::from(argument)
    } else {
        home.join(".ssh").join(argument)
    };
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };
    if !name.contains(['*', '?']) {
        return vec![path];
    }
    let pattern = name.to_string();
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| matches_glob(&pattern, name))
        })
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
}

fn matches_glob(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], n) || (!n.is_empty() && go(p, &n[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &n[1..]),
            (Some(a), Some(b)) if a == b => go(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    go(&p, &n)
}

/// The user's hosts: `~/.ssh/config` and the files it includes, one level.
pub fn hosts(home: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(home.join(".ssh").join("config")) else {
        return Vec::new();
    };
    let top = parse(&text);
    let mut hosts = top.hosts;
    for argument in &top.includes {
        for path in include_paths(argument, home) {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for name in parse(&text).hosts {
                if !hosts.contains(&name) {
                    hosts.push(name);
                }
            }
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_host_names_in_order_without_patterns() {
        let parsed = parse(
            "# my servers\n\
             Host dypod\n\
             \tHostName 203.0.113.7\n\
             \tUser ubuntu\n\
             Host web1 web2 *.internal\n\
             Host * !bastion\n\
             \tServerAliveInterval 30\n\
             Host=db  # the database\n\
             host \"quoted name\" box?\n\
             Match host foo\n\
             Host dypod\n",
        );
        assert_eq!(parsed.hosts, ["dypod", "web1", "web2", "db"]);
        assert!(parsed.includes.is_empty());
    }

    #[test]
    fn hosts_ssh_would_refuse_are_left_out() {
        let parsed = parse("Host -oProxyCommand=x good\n");
        assert_eq!(parsed.hosts, ["good"]);
    }

    #[test]
    fn includes_are_collected() {
        let parsed = parse("Include config.d/*\ninclude=~/work/ssh_config other\n");
        assert_eq!(
            parsed.includes,
            ["config.d/*", "~/work/ssh_config", "other"]
        );
    }

    #[test]
    fn globs_match_like_ssh() {
        assert!(matches_glob("*", "anything"));
        assert!(matches_glob("*.conf", "a.conf"));
        assert!(!matches_glob("*.conf", "a.conf.bak"));
        assert!(matches_glob("host?", "host1"));
        assert!(!matches_glob("host?", "host"));
    }

    #[test]
    fn hosts_follow_includes_one_level() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        fs::create_dir_all(ssh.join("config.d")).unwrap();
        fs::write(
            ssh.join("config"),
            "Include config.d/*.conf\nInclude ~/extra\nInclude missing\nHost main\n",
        )
        .unwrap();
        fs::write(ssh.join("config.d").join("b.conf"), "Host bee\n").unwrap();
        fs::write(
            ssh.join("config.d").join("a.conf"),
            "Host ay main\nInclude deeper\n",
        )
        .unwrap();
        fs::write(ssh.join("config.d").join("skip.txt"), "Host no\n").unwrap();
        fs::write(ssh.join("deeper"), "Host too-deep\n").unwrap();
        fs::write(home.path().join("extra"), "Host extra\n").unwrap();
        assert_eq!(hosts(home.path()), ["main", "ay", "bee", "extra"]);
    }

    #[test]
    fn no_config_is_no_hosts() {
        let home = tempfile::tempdir().unwrap();
        assert!(hosts(home.path()).is_empty());
    }
}
