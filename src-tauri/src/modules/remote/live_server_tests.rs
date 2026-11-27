//! The live server of a folder on a server: this machine serves it on
//! localhost, reading every file through the real agent served in-process,
//! and never a secret or anything outside the folder.

use terax_workspace_core::env::{SshHost, WorkspaceEnv};

use super::testing::connected;
use crate::modules::live_server::{read_target, served_folder, Failure, Served};
use crate::modules::workspace::WorkspaceRegistry;

#[test]
fn a_folder_on_the_server_is_served_from_this_machine() {
    let host: SshHost = connected("live-server");
    let env = WorkspaceEnv::Ssh { host };
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_string_lossy().replace('\\', "/");
    for sub in ["site/css", "site/.git"] {
        std::fs::create_dir_all(format!("{base}/{sub}")).unwrap();
    }
    std::fs::write(format!("{base}/site/page.html"), "<p>hi</p>").unwrap();
    std::fs::write(format!("{base}/site/css/a.css"), "p{}").unwrap();
    std::fs::write(format!("{base}/site/.git/config"), "PRIVATE").unwrap();
    std::fs::write(format!("{base}/site/id_rsa"), "PRIVATE").unwrap();
    let root = WorkspaceRegistry::default()
        .authorize_in(&env, &format!("{base}/site"))
        .unwrap();

    // An HTML file serves its folder.
    assert_eq!(
        served_folder(&format!("{root}/page.html"), &env).unwrap(),
        root
    );

    let get = |path: &str, segments: &[&str]| {
        let segments: Vec<String> = segments.iter().map(|s| s.to_string()).collect();
        read_target(&root, &env, path, &segments)
    };
    match get("/page.html", &["page.html"]) {
        Ok(Served::File { name, bytes }) => {
            assert_eq!(name, "page.html");
            assert_eq!(bytes, b"<p>hi</p>");
        }
        _ => panic!("expected the page"),
    }
    match get("/", &[]) {
        Ok(Served::Listing(html)) => {
            assert!(html.contains("page.html") && html.contains("css/"));
            assert!(!html.contains(".git"));
        }
        _ => panic!("expected a listing"),
    }
    assert!(matches!(get("/css", &["css"]), Ok(Served::Redirect(to)) if to == "/css/"));
    // Secrets inside the folder stay unread, even when asked for by name.
    assert!(matches!(get("/id_rsa", &["id_rsa"]), Err(Failure::Refused)));
    assert!(get("/.git/config", &[".git", "config"]).is_err());
}
