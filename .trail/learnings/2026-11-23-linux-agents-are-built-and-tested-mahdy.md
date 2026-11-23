---
title: "Linux agents are built and tested from the Windows machine with zig, and the ssh harness needs Git Bash's path rewriting turned off"
author: Mahdy
date: 2026-11-23
branch: dydev
cost: { minutes: 21, tokens: 51046 }
id: l-0405f93b
scope: repo
status: active
src: agent
touches:
  - docs/superpowers/plans/2026-11-23-remote-ssh-phase2-protocol-agent.md
  - docs/superpowers/specs/2026-11-22-remote-ssh-workspace-design.md
  - src-tauri/crates/terax-agent/tests/over_ssh.rs
  - src-tauri/crates/terax-workspace-core/src/host.rs
  - src-tauri/Cargo.lock
  - src-tauri/crates/terax-agent/Cargo.toml
  - src-tauri/crates/terax-agent/src/main.rs
  - src-tauri/crates/terax-agent/src/roots.rs
  - src-tauri/crates/terax-agent/tests/contract.rs
  - src-tauri/crates/terax-agent/tests/serve.rs
---

## Decided
- The Linux agents are cross-compiled on Windows with `cargo-zigbuild` (trial on 2026-11-23); the Windows installer can bundle what it builds itself, so it does not wait for CI artifacts.

## Watch out
- `cargo-zigbuild` looks for `zig` on `PATH` (or `python3 -m ziglang`, and Windows has no `python3`), so put the `ziglang` package directory on `PATH` first, or it fails with "Failed to find zig".
- zig's bundled musl lacks wrappers glibc has; `renameat2` failed to link, and the fix is to call the system call (`libc::syscall(libc::SYS_renameat2, ...)`), which glibc's wrapper only forwards to.
- Git Bash rewrites environment values that look like Unix paths: `TERAX_AGENT_SSH_PATH=/home/...` arrived as `C:/Program Files/Git/home/...`. Run the harness with `MSYS2_ENV_CONV_EXCL=TERAX_AGENT_SSH_PATH`, or from PowerShell.
- The agent serves with no arguments; the transport is `ssh -T <host> <path>/terax-agent`.

In the code: `crates/terax-agent/tests/over_ssh.rs` (the harness), `crates/terax-workspace-core/src/host.rs` (`rename_no_replace`). Build: `cargo zigbuild --release -p terax-agent --target x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`.
