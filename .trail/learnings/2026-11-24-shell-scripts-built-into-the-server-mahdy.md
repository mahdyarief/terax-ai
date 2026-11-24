---
title: Shell scripts built into the server agent must be written with Unix line ends
author: Mahdy
date: 2026-11-24
branch: dydev
cost: { minutes: 27, tokens: 237516 }
id: l-074be253
scope: repo
status: active
src: agent
touches:
  - src-tauri/crates/terax-agent/src/pty.rs
  - src-tauri/crates/terax-agent/tests/pty.rs
  - src-tauri/crates/terax-agent/tests/over_ssh.rs
---

## Decided
The agent writes the app's shell integration scripts on the server with every CRLF turned into LF, whatever the checkout it was built from.

## Tried and ruled out
Writing the `include_str!` content as it is: on dypod bash failed with "syntax error near unexpected token `elif'", because the agents are cross-built on Windows, where git checks the scripts out with CRLF. The local app never hit this, as a Unix build uses a Unix checkout.

## Watch out
- ConPTY asks for the cursor position (`ESC[6n`) when a terminal starts and prints nothing until it is answered. xterm.js answers it in the app; a test driving a terminal by hand must answer it (`ESC[1;1R`), or it times out with only `ESC[6n` read.
- Python heredocs in this repo's Git Bash have turned `\r` escapes into real CR bytes more than once. Rust normalizes CRLF inside a string literal to LF, so a `"\r\n"` written with a real CR silently became `"\n"`. Check written files for raw CR bytes.

In the code: `src-tauri/crates/terax-agent/src/pty.rs` (`integration::write`), `src-tauri/crates/terax-agent/tests/pty.rs` (`answer_cursor_query`), `tests/over_ssh.rs::a_terminal_runs_on_the_server_over_ssh`.
