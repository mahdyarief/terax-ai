//! Previews of a server's files: media grants read through the real agent
//! served in-process, ranged, and never outside what was granted.

use tauri::http::Request;
use terax_workspace_core::env::{SshHost, WorkspaceEnv};

use super::testing::connected;
use crate::modules::fs::assets::{respond, AssetGrants};
use crate::modules::workspace::WorkspaceRegistry;

struct Remote {
    _dir: tempfile::TempDir,
    root: String,
    outside: String,
    env: WorkspaceEnv,
}

/// `root/img/logo.png`, `root/docs/guide.pdf`, `root/.git/secret.svg`,
/// `root/page.html`, and a sibling `outside/private.png`, with `root`
/// authorized on host `name`.
fn remote(name: &str) -> Remote {
    let host: SshHost = connected(name);
    let env = WorkspaceEnv::Ssh { host };
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_string_lossy().replace('\\', "/");
    for sub in ["root/img", "root/docs", "root/.git", "outside"] {
        std::fs::create_dir_all(format!("{base}/{sub}")).unwrap();
    }
    std::fs::write(format!("{base}/root/img/logo.png"), "PNG_BYTES").unwrap();
    std::fs::write(format!("{base}/root/docs/guide.pdf"), "%PDF-1.7 body").unwrap();
    std::fs::write(format!("{base}/root/.git/secret.svg"), "PRIVATE").unwrap();
    std::fs::write(format!("{base}/root/page.html"), "<p>hi</p>").unwrap();
    std::fs::write(format!("{base}/outside/private.png"), "PRIVATE").unwrap();
    let root = WorkspaceRegistry::default()
        .authorize_in(&env, &format!("{base}/root"))
        .unwrap();
    let outside = format!("{}/outside", root.trim_end_matches("/root"));
    Remote {
        _dir: dir,
        root,
        outside,
        env,
    }
}

/// What the renderer asks for: `convertFileSrc(path)?token=…`.
fn request(path: &str, token: &str) -> Request<Vec<u8>> {
    let encoded =
        percent_encoding::utf8_percent_encode(path, percent_encoding::NON_ALPHANUMERIC).to_string();
    Request::builder()
        .uri(format!("https://asset.localhost/{encoded}?token={token}"))
        .body(Vec::new())
        .unwrap()
}

#[test]
fn an_image_and_a_pdf_on_the_server_are_served_to_their_view() {
    let r = remote("assets-media");
    let grants = AssetGrants::default();
    let logo = format!("{}/img/logo.png", r.root);
    let token = grants
        .grant("main".into(), &logo, r.env.clone(), 0)
        .unwrap();
    let served = respond(&grants, "main", &request(&logo, &token)).unwrap();
    assert_eq!(served.body(), b"PNG_BYTES");
    assert_eq!(served.headers()["Content-Type"], "image/png");
    // Another view cannot use the token.
    assert!(respond(&grants, "other", &request(&logo, &token)).is_err());

    // A PDF viewer reads it in ranges.
    let pdf = format!("{}/docs/guide.pdf", r.root);
    let token = grants.grant("main".into(), &pdf, r.env.clone(), 0).unwrap();
    let mut ranged = request(&pdf, &token);
    ranged
        .headers_mut()
        .insert("Range", "bytes=0-7".parse().unwrap());
    let served = respond(&grants, "main", &ranged).unwrap();
    assert_eq!(served.status(), 206);
    assert_eq!(served.body(), b"%PDF-1.7");
    assert_eq!(served.headers()["Content-Range"], "bytes 0-7/13");
    assert_eq!(served.headers()["Content-Type"], "application/pdf");
}

#[test]
fn a_folder_grant_serves_its_images_and_nothing_else() {
    let r = remote("assets-folder");
    let grants = AssetGrants::default();
    let token = grants
        .grant("main".into(), &r.root, r.env.clone(), 0)
        .unwrap();
    let logo = format!("{}/img/logo.png", r.root);
    assert!(respond(&grants, "main", &request(&logo, &token)).is_ok());
    // Climbing out of the granted tree, by `..` or by name.
    let climbed = format!("{}/img/../../outside/private.png", r.root);
    assert!(respond(&grants, "main", &request(&climbed, &token)).is_err());
    let outside = format!("{}/private.png", r.outside);
    assert!(respond(&grants, "main", &request(&outside, &token)).is_err());
    // A protected folder inside the grant, and a document that is not media.
    let secret = format!("{}/.git/secret.svg", r.root);
    assert!(respond(&grants, "main", &request(&secret, &token)).is_err());
    // Opened by the user directly, as on this machine, it is shown.
    let direct = grants
        .grant("main".into(), &secret, r.env.clone(), 0)
        .unwrap();
    assert_eq!(
        respond(&grants, "main", &request(&secret, &direct))
            .unwrap()
            .body(),
        b"PRIVATE"
    );
    let page = format!("{}/page.html", r.root);
    assert!(respond(&grants, "main", &request(&page, &token)).is_err());
    // Nor can a grant be made outside the authorized folder.
    assert!(grants
        .grant("main".into(), &outside, r.env.clone(), 0)
        .is_err());
}
