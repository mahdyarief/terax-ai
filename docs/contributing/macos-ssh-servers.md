# TODO: a Mac as an SSH server

Status: **not started; contributions welcome.** Needs a Mac you can reach over SSH to test against.

Terax on macOS already works as an SSH *client*: a Mac connects to Linux servers. This TODO is the other direction: opening a space on a Mac **as the server**, the way a Linux server works today.

Today a Mac server is refused on purpose, in two places:

- the bootstrap refuses any `uname -s` other than `Linux`, with "Darwin servers are not supported; Terax runs on Linux servers";
- the agent's file watcher refuses on macOS, with "watching is not available on a macOS server".

The rest of the agent (file calls, git, terminals through `portable-pty`, the protocol) is portable Rust. On macOS CI those code paths have not been exercised as a server, so expect a few surprises.

## Work

### 1. Bootstrap accepts Darwin

- [ ] `src-tauri/crates/terax-remote-protocol/src/bootstrap.rs`
  - Accept `os=Darwin` next to `Linux` in `plan` (the refusal is near the `report.os != "Linux"` check).
  - Key the agent platforms by OS **and** arch. Today the table maps `uname -m` only to `x86_64-linux-musl` and `aarch64-linux-musl`. Add:
    - `Darwin` + `x86_64` → `x86_64-apple-darwin`;
    - `Darwin` + `arm64` → `aarch64-apple-darwin`.
  - Update the unit test that asserts a Darwin report is unsupported.
- [ ] `src-tauri/src/modules/remote_agent.rs`: add the two platforms to `AGENT_PLATFORMS`. `cargo_target` only rewrites `-linux-`, so `x86_64-apple-darwin` and `aarch64-apple-darwin` already pass through unchanged. Add a test for that.
- [ ] The bootstrap script is POSIX `sh`. Check it on macOS's `/bin/sh` and with zsh as the login shell (`terax-remote-protocol/tests/bootstrap_sh.rs` runs it "in every shell").
- [ ] The bootstrap script hashes the installed agent only with `sha256sum` (`hasher=...` in `bootstrap.rs`). macOS may not have it but has `shasum -a 256`; without a hasher the installed agent cannot be verified for reuse. Add `shasum -a 256` as a fallback.
- [ ] Check the rest of the script for other Linux-only commands or flags (`stat`, `mktemp`, `cut`).

### 2. Build and ship the macOS agents (the hard part)

The Linux agents are static musl binaries made with `cargo zigbuild` (`scripts/build-agent.mjs`). They can be built on Windows, macOS or Linux and are **bundled into every installer**.

macOS agents are different:

- they need Apple's SDK, so build them on a macOS machine or runner;
- binaries for Apple Silicon must be signed (an ad-hoc signature is enough; the linker adds one when building on macOS);
- the Windows installer is built locally with `pnpm build:nosign` and cannot build them.

Options:

- [ ] **A (recommended):** the macOS job in `.github/workflows/release-dy.yml` builds both darwin agents and uploads them to the release. The app downloads the one it needs (pinned by version, checked by SHA-256) the first time it connects to a Mac, instead of bundling it. Installers stay small, and a Windows build needs no Mac.
- [ ] **B:** bundle them like the Linux agents. Every installer then needs the darwin binaries at build time: download them from CI before `tauri build`, or build every installer on CI.

Either way:

- [ ] add `x86_64-apple-darwin` and `aarch64-apple-darwin` to `ALL_PLATFORMS` in `scripts/build-agent.mjs` (with plain `cargo build`, not zigbuild, on macOS);
- [ ] decide whether the agent's SHA check and the installed-agent reuse (`a_first_connect_installs_and_a_second_reuses`) cover the downloaded case.

### 3. File watching on a Mac server

Without a watcher, the explorer and source control on a Mac server don't refresh when files change outside the app (the editor still saves and reloads).

- [ ] `src-tauri/crates/terax-agent/src/watch.rs`: replace the `#[cfg(target_os = "macos")] fn start` that returns `Unsupported`.
- [ ] The app already has an FSEvents backend for local Mac folders: `src-tauri/src/modules/fs/watch/macos.rs` (about 500 lines, `fsevent-sys`). Move it into `terax-workspace-core::watch` (beside `portable.rs`, which serves inotify and ReadDirectoryChangesW). The app and the agent then share it.
- [ ] Keep the macOS tests in `fs/watch/macos.rs` passing after the move.

### 4. Tests and CI

These tests run the agent as a server on the CI machine itself. They are ignored on macOS today with `#[cfg_attr(target_os = "macos", ignore = "the agent runs on Linux servers and refuses macOS")]`. Re-enable them:

- [ ] `crates/terax-agent/tests/transport.rs`: `a_first_connect_installs_and_a_second_reuses`, `shell_noise_is_skipped_and_stderr_reaches_the_log`, `a_session_that_dies_mid_install_fails_cleanly`, `a_server_without_an_agent_for_it_is_refused_and_left_alone`.
- [ ] `crates/terax-remote-protocol/tests/bootstrap_sh.rs`: `a_first_connect_installs_and_a_second_reuses_in_every_shell` (it asserts `os == "Linux"`).
- [ ] `crates/terax-agent/tests/serve.rs`: `watched_folders_and_files_report_changes_until_the_set_is_emptied` (needs step 3).
- [ ] `src/modules/remote/watch_tests.rs`: `a_watched_server_folder_reports_changes_and_is_watched_again_after_a_reconnect` (needs step 3).
- [ ] `src/modules/remote/git_tests.rs`: `a_commit_made_on_the_server_reaches_source_control` (needs step 3).
- [ ] `src/modules/pty/remote.rs`: `a_server_terminal_runs_through_the_apps_session`.
  - **Not understood yet.** On macOS it timed out ("a response within 10 s"), while the other three tests in that module passed.
  - Find the cause before re-enabling: it may be a real client-side problem, not only the server side.

These stay ignored on macOS for good, because APFS refuses a name that is not valid UTF-8, so the case cannot happen there:
`host::tests::a_canonical_path_that_is_not_unicode_is_refused`, `workspace::auth_tests::a_non_unicode_sibling_cannot_alias_an_authorized_root`, `workspace::auth_tests::a_path_that_cannot_be_spelled_exactly_is_never_covered`, `fs::assets::tests::a_link_into_a_lossy_sibling_is_not_inside_a_media_grant`.

### 5. End to end on a real Mac

- [ ] On the Mac: System Settings → General → Sharing → **Remote Login** on (macOS ships with SSH off).
- [ ] From Terax on Windows, Linux and macOS: connect, open a folder, and check:
  - the explorer;
  - the editor (save, and a change made outside the app);
  - source control;
  - a terminal (zsh is the default shell; check shell integration and `cd` following);
  - an image paste (`~/.cache/terax-image`);
  - search and the AI tools;
  - a reconnect after sleep.
- [ ] Test with password, key and passphrase logins (askpass must keep working).
- [ ] Gatekeeper: a binary copied over SSH carries no quarantine attribute, so it should run. Confirm on a current macOS.

## Done when

- A Mac server connects and works like a Linux one, from every client platform.
- The tests above run on macOS CI, except the four APFS ones.
- The user docs (TERAX.md) say macOS servers are supported and that Remote Login must be on.
