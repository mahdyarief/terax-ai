//! Live end-to-end test for the full-app web remote bridge.
//!
//! `#[ignore]` by default. Run on Linux (aspirexc) after building the binary:
//! `cargo test --lib -- web_remote::app::live_tests --ignored --nocapture`

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use sha2::{Digest, Sha256};

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn terax_bin() -> PathBuf {
        if let Ok(p) = std::env::var("TERAX_BIN") {
            return PathBuf::from(p);
        }
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("release")
            .join("terax")
    }

    fn free_port() -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    }

    fn http_req(
        port: u16,
        method: &str,
        path: &str,
        token: &str,
        body: Option<&[u8]>,
    ) -> (u16, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let bytes = body.unwrap_or(&[]);
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\nX-Terax-Remote: 1\r\nCookie: terax_remote={token}\r\nConnection: close\r\n"
        );
        if body.is_some() {
            req.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                bytes.len()
            ));
        }
        req.push_str("\r\n");
        s.write_all(req.as_bytes()).unwrap();
        if body.is_some() {
            s.write_all(bytes).unwrap();
        }
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&raw[..split]);
        let status: u16 = head
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        (status, raw[split + 4..].to_vec())
    }

    #[test]
    #[ignore]
    fn live_remote_app_invokes_commands_and_relays_fs_changed() {
        let bin = terax_bin();
        assert!(bin.exists(), "missing binary at {}", bin.display());

        let data_dir = tempfile::tempdir().unwrap();
        let app_dir = data_dir.path().join("app.crynta.terax");
        std::fs::create_dir_all(&app_dir).unwrap();

        let token = "live-test-token-0123456789abcdef";
        let hash: String = Sha256::digest(token.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let devices_json = serde_json::json!({
            "version": 1,
            "devices": [{
                "id": "live-dev",
                "name": "live-test",
                "tokenSha256": hash,
                "grant": { "input": true, "tabs": true, "app": true },
                "createdAt": 1,
                "lastSeen": 1
            }]
        });
        std::fs::write(
            app_dir.join("remote-devices.json"),
            serde_json::to_vec_pretty(&devices_json).unwrap(),
        )
        .unwrap();

        let watch_dir = tempfile::tempdir().unwrap();
        let port = free_port();
        let child = Command::new("xvfb-run")
            .args([
                "-a",
                bin.to_str().unwrap(),
                "--remote-serve",
                "--bind",
                "loopback",
                "--port",
                &port.to_string(),
            ])
            .env("XDG_DATA_HOME", data_dir.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn xvfb-run terax");
        let _guard = ChildGuard(child);

        // Wait for server to be ready.
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if Instant::now() >= deadline {
                panic!("timed out waiting for terax --remote-serve on port {port}");
            }
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                let (status, _) = http_req(port, "GET", "/api/me", token, None);
                if status == 200 {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }

        // 1. Create relay session.
        let (status, body) = http_req(port, "POST", "/api/app/session", token, Some(b"{}"));
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
        let sess: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = sess["id"].as_str().unwrap().to_string();

        // 2. Invoke pty_list_shells and plugin:store|load.
        let (status, body) = http_req(
            port,
            "POST",
            "/api/app/invoke",
            token,
            Some(
                serde_json::to_vec(&serde_json::json!({
                    "session": session_id,
                    "cmd": "pty_list_shells",
                    "args": {}
                }))
                .unwrap()
                .as_slice(),
            ),
        );
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
        let shells: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(shells.is_array(), "{shells}");

        let (status, body) = http_req(
            port,
            "POST",
            "/api/app/invoke",
            token,
            Some(
                serde_json::to_vec(&serde_json::json!({
                    "session": session_id,
                    "cmd": "plugin:store|load",
                    "args": { "path": "live-test-settings.json" }
                }))
                .unwrap()
                .as_slice(),
            ),
        );
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

        // 3. Open callback SSE stream, listen for fs:changed, touch a file.
        let mut sse = TcpStream::connect(("127.0.0.1", port)).unwrap();
        sse.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let sse_req = format!(
            "GET /api/app/callbacks?session={session_id} HTTP/1.1\r\nHost: localhost:{port}\r\nCookie: terax_remote={token}\r\n\r\n"
        );
        sse.write_all(sse_req.as_bytes()).unwrap();

        let (status, body) = http_req(
            port,
            "POST",
            "/api/app/invoke",
            token,
            Some(
                serde_json::to_vec(&serde_json::json!({
                    "session": session_id,
                    "cmd": "plugin:event|listen",
                    "args": {
                        "event": "fs:changed",
                        "target": { "kind": "Any" },
                        "handler": 888001
                    }
                }))
                .unwrap()
                .as_slice(),
            ),
        );
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

        let watch_path = watch_dir.path().to_string_lossy().into_owned();
        let (status, body) = http_req(
            port,
            "POST",
            "/api/app/invoke",
            token,
            Some(
                serde_json::to_vec(&serde_json::json!({
                    "session": session_id,
                    "cmd": "workspace_authorize",
                    "args": { "path": watch_path }
                }))
                .unwrap()
                .as_slice(),
            ),
        );
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

        let (status, body) = http_req(
            port,
            "POST",
            "/api/app/invoke",
            token,
            Some(
                serde_json::to_vec(&serde_json::json!({
                    "session": session_id,
                    "cmd": "fs_watch_add",
                    "args": { "paths": [watch_path] }
                }))
                .unwrap()
                .as_slice(),
            ),
        );
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(watch_dir.path().join("touched.txt"), b"hi").unwrap();

        let mut buf = [0u8; 4096];
        let mut collected = String::new();
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline && !collected.contains("888001") {
            match sse.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => collected.push_str(&String::from_utf8_lossy(&buf[..n])),
                Err(_) => break,
            }
        }
        assert!(
            collected.contains("888001") && collected.contains("fs:changed"),
            "expected fs:changed callback on 888001, got:\n{collected}"
        );
    }

    fn raw_post(
        port: u16,
        path: &str,
        token: &str,
        extra_headers: &[(&str, &str)],
        body: &[u8],
    ) -> (u16, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        let mut req = format!(
            "POST {path} HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\nX-Terax-Remote: 1\r\nCookie: terax_remote={token}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for (k, v) in extra_headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");
        s.write_all(req.as_bytes()).unwrap();
        s.write_all(body).unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&raw[..split]);
        let status: u16 = head
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        (status, raw[split + 4..].to_vec())
    }

    fn encode_pty_req(stream_token: u64, ack: Option<(u32, u64)>) -> Vec<u8> {
        let mut out = Vec::with_capacity(32);
        out.extend_from_slice(&stream_token.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes()); // epoch
        if let Some((id, bytes)) = ack {
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&bytes.to_le_bytes());
        } else {
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        out
    }

    #[test]
    #[ignore]
    fn live_remote_app_pty_stream_under_load() {
        let bin = terax_bin();
        assert!(bin.exists(), "missing binary at {}", bin.display());

        let data_dir = tempfile::tempdir().unwrap();
        let app_dir = data_dir.path().join("app.crynta.terax");
        std::fs::create_dir_all(&app_dir).unwrap();

        let token = "live-pty-token-0123456789abcdef";
        let hash: String = Sha256::digest(token.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let devices_json = serde_json::json!({
            "version": 1,
            "devices": [{
                "id": "live-pty-dev",
                "name": "live-pty-test",
                "tokenSha256": hash,
                "grant": { "input": true, "tabs": true, "app": true },
                "createdAt": 1,
                "lastSeen": 1
            }]
        });
        std::fs::write(
            app_dir.join("remote-devices.json"),
            serde_json::to_vec_pretty(&devices_json).unwrap(),
        )
        .unwrap();

        let port = free_port();
        let child = Command::new("xvfb-run")
            .args([
                "-a",
                bin.to_str().unwrap(),
                "--remote-serve",
                "--bind",
                "loopback",
                "--port",
                &port.to_string(),
            ])
            .env("XDG_DATA_HOME", data_dir.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn xvfb-run terax");
        let _guard = ChildGuard(child);

        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if Instant::now() >= deadline {
                panic!("timed out waiting for terax --remote-serve on port {port}");
            }
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                let (status, _) = http_req(port, "GET", "/api/me", token, None);
                if status == 200 {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }

        let (status, body) = http_req(port, "POST", "/api/app/session", token, Some(b"{}"));
        assert_eq!(status, 200);
        let sess: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = sess["id"].as_str().unwrap().to_string();

        let (status, body) = http_req(
            port,
            "POST",
            "/api/app/invoke",
            token,
            Some(
                serde_json::to_vec(&serde_json::json!({
                    "session": session_id,
                    "cmd": "pty_stream_open",
                    "args": {}
                }))
                .unwrap()
                .as_slice(),
            ),
        );
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
        let token_hex: String = serde_json::from_slice(&body).unwrap();
        let stream_token = u64::from_str_radix(&token_hex, 16).unwrap();

        let (status, body) = http_req(
            port,
            "POST",
            "/api/app/invoke",
            token,
            Some(
                serde_json::to_vec(&serde_json::json!({
                    "session": session_id,
                    "cmd": "pty_open",
                    "args": { "cols": 80, "rows": 24 }
                }))
                .unwrap()
                .as_slice(),
            ),
        );
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
        let pty_info: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let pty_id = pty_info["id"].as_u64().unwrap() as u32;
        let write_token = pty_info["token"].as_str().unwrap().to_string();

        // Announce receiver to hub via /scheme/terax-pty/<session>/ack.
        let ack_path = format!("/scheme/terax-pty/{session_id}/ack");
        let poll_path = format!("/scheme/terax-pty/{session_id}/poll");
        let (status, body) = raw_post(
            port,
            &ack_path,
            token,
            &[],
            &encode_pty_req(stream_token, Some((pty_id, 0))),
        );
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

        // Write `yes | head -c 52428800` (50 MiB) using raw invoke + X-Terax-Invoke-Headers.
        let hdr_json = serde_json::json!({
            "x-pty-id": pty_id.to_string(),
            "x-pty-token": write_token,
            "x-pty-epoch": "0",
        })
        .to_string();
        let (status, body) = raw_post(
            port,
            "/api/app/invoke",
            token,
            &[
                ("X-Terax-Session", &session_id),
                ("X-Terax-Cmd", "pty_write"),
                ("X-Terax-Invoke-Headers", &hdr_json),
            ],
            b"yes | head -c 52428800\n",
        );
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

        let mut received: u64 = 0;
        let load_deadline = Instant::now() + Duration::from_secs(60);
        while received < 52_428_800 && Instant::now() < load_deadline {
            let (status, frame_buf) = raw_post(
                port,
                &poll_path,
                token,
                &[],
                &encode_pty_req(stream_token, Some((pty_id, received))),
            );
            assert_eq!(status, 200, "{}", String::from_utf8_lossy(&frame_buf));
            let mut pos = 0;
            while pos + 9 <= frame_buf.len() {
                let id = u32::from_le_bytes(frame_buf[pos..pos + 4].try_into().unwrap());
                let kind = frame_buf[pos + 4];
                let len = u32::from_le_bytes(frame_buf[pos + 5..pos + 9].try_into().unwrap()) as usize;
                pos += 9;
                if pos + len > frame_buf.len() {
                    break;
                }
                if id == pty_id && kind == 1 && len >= 8 {
                    let start_off =
                        u64::from_le_bytes(frame_buf[pos..pos + 8].try_into().unwrap());
                    let data_len = (len - 8) as u64;
                    if start_off + data_len > received {
                        received = start_off + data_len;
                    }
                }
                pos += len;
            }
        }
        assert!(
            received >= 52_428_800,
            "expected >= 52428800 bytes from yes | head -c 50M, got {received}"
        );

        let _ = http_req(
            port,
            "POST",
            "/api/app/invoke",
            token,
            Some(
                serde_json::to_vec(&serde_json::json!({
                    "session": session_id,
                    "cmd": "pty_close",
                    "args": { "id": pty_id }
                }))
                .unwrap()
                .as_slice(),
            ),
        );
    }
}
