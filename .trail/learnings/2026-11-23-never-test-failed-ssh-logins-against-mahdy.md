---
title: "Never test failed SSH logins against dypod: its fail2ban bans this machine"
author: Mahdy
date: 2026-11-23
branch: dydev
cost: { minutes: 1, tokens: 1869 }
id: l-3870b0c3
scope: repo
status: active
src: agent
---

## Watch out
- dypod (the Ubuntu test server) runs fail2ban. A few deliberately failed logins (`Permission denied (publickey)`, as from a test that answers a wrong passphrase or offers an unknown key) banned this machine's IP: port 22 went filtered and every ssh timed out. Nothing on the server had changed; the ban lifts by itself after its ban time.
- So a test against a real server must succeed at logging in. To exercise a passphrase prompt, authorize a throwaway key for the test's run (restricted with `restrict,command="true"`, removed on drop) and answer its right passphrase.
- OpenSSH asks for a key's passphrase only after the server accepts the key: the public half is read without decrypting (from the `.pub` file, or from the new key format's plain header), and an unknown key is never decrypted. The host's own `IdentityFile` lines are added even with `-i` and `IdentitiesOnly=yes`; use `ssh -G <host>` for its address, then `-F none`.

## Decided
- Don't retry ssh while banned: each attempt can extend the ban. Probe the port with a plain TCP connect instead.

In the code:
- `src-tauri/crates/terax-cli/tests/askpass_over_ssh.rs` (ignored; `TERAX_AGENT_SSH_HOST=dypod`)
