//! Optional HTTPS through `tailscale serve`.
//!
//! Tailscale terminates TLS on 443 and proxies to the loopback listener, so the
//! browser gets a real certificate and the cookie can be `Secure`. Terax only
//! configures serve when nothing else has: a serve config it did not create is
//! left untouched, and stopping takes only its own mount away.

use std::process::{Command, Output};

/// The HTTPS port `tailscale serve` listens on.
pub const HTTPS_PORT: u16 = 443;

/// Runs the CLI off the async runtime: it is a short, blocking call.
async fn tailscale(args: &[&str]) -> Result<Output, String> {
    let owned: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    tauri::async_runtime::spawn_blocking(move || {
        Command::new("tailscale")
            .args(&owned)
            .output()
            .map_err(|error| format!("Could not run tailscale: {error}"))
    })
    .await
    .map_err(|error| format!("tailscale task failed: {error}"))?
}

fn failure(output: &Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if text.is_empty() {
        format!("tailscale exited with {}", output.status)
    } else {
        text
    }
}

/// Whether a serve configuration already exists. Terax refuses to add to one:
/// overwriting someone's handlers would break whatever it was serving.
pub fn serve_conflict(status_json: &str) -> bool {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(status_json) else {
        return false;
    };
    for key in ["Web", "TCP"] {
        if let Some(map) = json.get(key).and_then(|value| value.as_object()) {
            if !map.is_empty() {
                return true;
            }
        }
    }
    false
}

/// `Self.DNSName` from `tailscale status --json`, without its trailing dot.
pub fn parse_magic_dns(status_json: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(status_json).ok()?;
    let name = json.get("Self")?.get("DNSName")?.as_str()?;
    let name = name.trim_end_matches('.');
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// This machine's MagicDNS name, which is what the browser opens.
pub async fn magic_dns() -> Result<String, String> {
    let output = tailscale(&["status", "--json"]).await?;
    if !output.status.success() {
        return Err(failure(&output));
    }
    parse_magic_dns(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "Tailscale did not report a DNS name for this machine.".to_string())
}

/// Claims `https://<magic dns>/` for the loopback listener. Refuses when any
/// serve configuration already exists.
pub async fn claim(target: &str) -> Result<(), String> {
    let status = tailscale(&["serve", "status", "--json"]).await?;
    if status.status.success() && serve_conflict(&String::from_utf8_lossy(&status.stdout)) {
        return Err(
            "Tailscale serve is already configured on this machine. Remove it first.".to_string(),
        );
    }
    let port = format!("--https={HTTPS_PORT}");
    let served = tailscale(&["serve", "--bg", &port, target]).await?;
    if !served.status.success() {
        return Err(failure(&served));
    }
    Ok(())
}

/// Removes only the mount Terax added.
pub async fn release() -> Result<(), String> {
    let port = format!("--https={HTTPS_PORT}");
    let output = tailscale(&["serve", &port, "off"]).await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(failure(&output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_status_has_nothing_to_conflict_with() {
        assert!(!serve_conflict("{}"));
        assert!(!serve_conflict(r#"{"Web":{},"TCP":{}}"#));
        assert!(!serve_conflict("not json"));
    }

    #[test]
    fn any_existing_handler_is_a_conflict() {
        assert!(serve_conflict(
            r#"{"Web":{"box.tail1.ts.net:443":{"Handlers":{}}}}"#
        ));
        assert!(serve_conflict(r#"{"TCP":{"443":{"HTTPS":true}}}"#));
    }

    #[test]
    fn reads_the_magic_dns_name_without_the_trailing_dot() {
        let status = r#"{"Self":{"DNSName":"box.tail1.ts.net."}}"#;
        assert_eq!(parse_magic_dns(status).as_deref(), Some("box.tail1.ts.net"));
        assert_eq!(parse_magic_dns(r#"{"Self":{}}"#), None);
        assert_eq!(parse_magic_dns("garbage"), None);
    }
}
