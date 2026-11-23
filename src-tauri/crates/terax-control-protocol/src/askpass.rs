//! OpenSSH's questions (passwords, passphrases, host keys), asked in Terax.
//!
//! `ssh` runs `terax-cli` as its `SSH_ASKPASS` program with the prompt as its
//! argument; the CLI sends `askpass` to the app, which shows a dialog and
//! returns the answer. The method is only accepted with a per-connection
//! askpass token, never with the app's token or a terminal's.

use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const METHOD_ASKPASS: &str = "askpass";

/// How long the user has to answer.
pub const ASKPASS_WAIT: Duration = Duration::from_secs(300);

/// Set in `ssh`'s environment so `terax-cli` acts as its askpass program.
pub const ENV_ASKPASS: &str = "TERAX_SSH_ASKPASS";

/// The error code of a dismissed dialog.
pub const CANCELLED: &str = "cancelled";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct AskpassParams {
    /// OpenSSH's text, verbatim (it may span lines).
    pub prompt: String,
    /// `SSH_ASKPASS_PROMPT`: `confirm` (answered by exit status) or `none`
    /// (a notice, such as "touch your security key").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// What the app sends its dialog: the host, the prompt and what to show.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct AskpassQuestion {
    pub host: String,
    pub prompt: String,
    pub kind: AskpassKind,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AskpassKind {
    /// A password, passphrase, PIN or code: typed, masked.
    Secret,
    /// Answered `yes` or `no` in text; `host_key` when it is about trusting
    /// a host key seen for the first time.
    YesNo { host_key: bool },
    /// A host key that differs from the known one. It cannot be accepted
    /// from the dialog; the user fixes `known_hosts` knowingly.
    ChangedHostKey,
    /// Allow or deny (ssh-agent's confirmation), answered by exit status.
    Confirm,
    /// Information only; nothing to answer.
    Notice,
}

/// The dialog's answer, as the app's response result.
#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
pub struct AskpassAnswer {
    /// The text for `ssh` (a secret, or `yes`/`no`); empty for `Confirm`
    /// and `Notice`.
    #[serde(default)]
    pub answer: String,
    /// For `Confirm`: whether the user allowed it.
    #[serde(default)]
    pub confirmed: bool,
}

impl std::fmt::Debug for AskpassAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AskpassAnswer")
            .field("answer", &"<redacted>")
            .field("confirmed", &self.confirmed)
            .finish()
    }
}

/// What kind of question OpenSSH is asking, from its text and hint.
pub fn classify(prompt: &str, hint: Option<&str>) -> AskpassKind {
    match hint {
        Some("confirm") => return AskpassKind::Confirm,
        Some("none") => return AskpassKind::Notice,
        _ => {}
    }
    let lower = prompt.to_ascii_lowercase();
    if lower.contains("remote host identification has changed")
        || lower.contains("possible dns spoofing")
        || lower.contains("host key verification failed")
    {
        return AskpassKind::ChangedHostKey;
    }
    if lower.contains("(yes/no") || lower.contains("please type 'yes', 'no'") {
        let host_key = lower.contains("authenticity of host")
            || lower.contains("key fingerprint is")
            || lower.contains("host key");
        return AskpassKind::YesNo { host_key };
    }
    AskpassKind::Secret
}

#[cfg(test)]
mod tests {
    use super::*;

    // Texts as OpenSSH 9.x prints them.
    const NEW_HOST_KEY: &str =
        "The authenticity of host 'dypod (203.0.113.7)' can't be established.\n\
        ED25519 key fingerprint is SHA256:Xb2ZyS0pKcY4hFkq3l0m9g8n7o6p5q4r3s2t1u0v9w8.\n\
        This key is not known by any other names.\n\
        Are you sure you want to continue connecting (yes/no/[fingerprint])? ";

    #[test]
    fn secrets_are_masked() {
        for prompt in [
            "me@dypod's password: ",
            "Enter passphrase for key '/home/me/.ssh/id_ed25519': ",
            "Enter passphrase for key 'C:\\Users\\me/.ssh/id_ed25519': ",
            "(me@dypod) Password: ",
            "(me@dypod) Verification code: ",
            "Enter PIN for ED25519-SK key /home/me/.ssh/id_ed25519_sk: ",
            "Password: ",
        ] {
            assert_eq!(classify(prompt, None), AskpassKind::Secret, "{prompt}");
        }
    }

    #[test]
    fn a_new_host_key_is_a_yes_no_about_the_key() {
        assert_eq!(
            classify(NEW_HOST_KEY, None),
            AskpassKind::YesNo { host_key: true }
        );
        assert_eq!(
            classify("Please type 'yes', 'no' or the fingerprint: ", None),
            AskpassKind::YesNo { host_key: false }
        );
        let ip_mismatch = "Warning: the ECDSA host key for 'dypod' differs from the key for the IP address '203.0.113.7'\n\
            Offending key for IP in /home/me/.ssh/known_hosts:3\n\
            Are you sure you want to continue connecting (yes/no)? ";
        assert_eq!(
            classify(ip_mismatch, None),
            AskpassKind::YesNo { host_key: true }
        );
    }

    #[test]
    fn a_changed_host_key_is_never_a_plain_question() {
        let changed = "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
            @    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n\
            @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
            IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!\n\
            Are you sure you want to continue connecting (yes/no)? ";
        assert_eq!(classify(changed, None), AskpassKind::ChangedHostKey);
        assert_eq!(
            classify("Warning: POSSIBLE DNS SPOOFING DETECTED! (yes/no)? ", None),
            AskpassKind::ChangedHostKey
        );
    }

    #[test]
    fn the_hint_decides_confirmations_and_notices() {
        assert_eq!(
            classify(
                "Allow use of key /home/me/.ssh/id_ed25519?\nKey fingerprint SHA256:abc.",
                Some("confirm")
            ),
            AskpassKind::Confirm
        );
        assert_eq!(
            classify(
                "Confirm user presence for key ED25519-SK SHA256:abc",
                Some("none")
            ),
            AskpassKind::Notice
        );
        assert_eq!(
            classify("me@dypod's password: ", Some("")),
            AskpassKind::Secret
        );
    }

    #[test]
    fn the_wire_shapes() {
        let question = AskpassQuestion {
            host: "dypod".into(),
            prompt: "p".into(),
            kind: AskpassKind::YesNo { host_key: true },
        };
        assert_eq!(
            serde_json::to_value(&question).unwrap(),
            serde_json::json!({ "host": "dypod", "prompt": "p", "kind": { "yes_no": { "host_key": true } } })
        );
        assert_eq!(
            serde_json::to_value(AskpassKind::Secret).unwrap(),
            serde_json::json!("secret")
        );
        let params: AskpassParams =
            serde_json::from_value(serde_json::json!({ "prompt": "x" })).unwrap();
        assert_eq!(params.hint, None);
    }

    #[test]
    fn an_answer_never_prints() {
        let answer = AskpassAnswer {
            answer: "hunter2".into(),
            confirmed: false,
        };
        assert!(!format!("{answer:?}").contains("hunter2"));
    }
}
