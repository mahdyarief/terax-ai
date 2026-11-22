//! The workspace core also builds the remote agent, a small binary with no
//! UI and no async runtime, so it must not depend on the app, Tauri or Tokio.

const FORBIDDEN: [&str; 3] = ["tauri", "tokio", "terax"];

/// Dependency names from every `[...dependencies]` table of a manifest
/// (also target-specific ones and `[dependencies.name]` tables).
fn dependency_names(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_dependencies = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            let header = header.trim_end_matches(']');
            if let Some((_, name)) = header.rsplit_once("dependencies.") {
                names.push(name.trim_matches('"').to_string());
                in_dependencies = false;
            } else {
                in_dependencies = header.ends_with("dependencies");
            }
            continue;
        }
        if in_dependencies {
            if let Some((name, _)) = line.split_once('=') {
                names.push(name.trim().trim_matches('"').to_string());
            }
        }
    }
    names
}

fn forbidden(name: &str) -> bool {
    FORBIDDEN
        .iter()
        .any(|bad| name == *bad || name.starts_with(&format!("{bad}-")) || name.starts_with(&format!("{bad}_")))
        && name != "terax-workspace-core"
}

#[test]
fn the_core_depends_on_neither_the_app_nor_tauri_nor_tokio() {
    let manifest = include_str!("../Cargo.toml");
    let names = dependency_names(manifest);
    assert!(names.iter().any(|n| n == "ignore"), "the parse found {names:?}");
    let bad: Vec<_> = names.iter().filter(|n| forbidden(n)).collect();
    assert!(bad.is_empty(), "forbidden core dependencies: {bad:?}");
}

#[test]
fn the_check_catches_each_kind_of_dependency() {
    let manifest = r#"
[package]
name = "x"

[dependencies]
serde = "1"
tauri = { version = "2" }

[target.'cfg(unix)'.dependencies]
tokio = "1"

[dependencies.terax]
path = "../.."

[dev-dependencies]
tauri-plugin-fs = "2"
"#;
    let names = dependency_names(manifest);
    let bad: Vec<_> = names.iter().filter(|n| forbidden(n)).cloned().collect();
    assert_eq!(bad, ["tauri", "tokio", "terax", "tauri-plugin-fs"]);
    assert!(!forbidden("serde") && !forbidden("tokenizers"));
}
