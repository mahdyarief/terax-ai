# Task: implement the Terax "Web Remote" plan

You are working in the Terax repo (a Tauri 2 desktop terminal: Rust backend in `src-tauri/`, React/TS frontend in `src/`, Ghostty WASM renderer in `packages/ghostty-core`). Working copy: `D:\Github\agent-workspace\terax-ai`, fork `mahdyarief/terax-ai`, branch **`dydev`**.

## What to build
Follow **`docs/superpowers/plans/2026-11-27-web-remote.md`** exactly. Read it fully before starting, including the "Why this works" and "Decisions" sections. Goal: from a browser (`http://<host>:<port>/web-index.html`, usually over Tailscale), watch every terminal tab of every workspace live, in terminal or block mode, and type into them or open and close tabs when the paired device has the grant.

Work through Tasks 1 to 19 in order. If the code contradicts the plan, stop and say so; do not change a "Decision" on your own. Ask before:
- adding a dependency the plan does not name;
- weakening a security rule (bind default, pairing, grants, Host/Origin checks, CSP).

## How to work
- **One task = one commit**, test first where it adds behaviour (RED, then GREEN). After each finished task, **commit and push to `dydev` without asking**.
- Commit messages end with:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
- Tick the task's checkboxes in the plan file in the same commit. When a phase is done, update the plan's **Status** line.
- **Never** merge to `main`, tag, create a release, or open a PR. Ask first.
- Do not trigger GitHub Actions. Do not push to `main`.
- Before each commit, run `trail check`. After a task that settles a non-obvious decision or a ruled-out approach, write a learning with `trail note --title "<decision in plain words>"`. The text goes on stdin with the sections `## Decided`, `## Tried and ruled out`, `## Watch out`, then an `In the code:` line. Commit the learning with the change.

## Hard rules
- Package manager: **pnpm only** (`pnpm exec …`). Never npm, npx or yarn.
- Never touch the installed `D:\Terax\terax.exe`. To run the app, use `pnpm dev:app`.
- Formatting:
  - Never run `cargo fmt` on the crate. `rustfmt --edition 2021 <file>` is fine on new files only.
  - Never run `biome format --write` on existing files you did not otherwise change wholesale. To check an existing file, compare biome's findings against HEAD and only fix what you introduced. New files may be formatted.
  - Working copies are CRLF, so biome reports "format" on almost every file; that is noise. Only `lint/…` and `assist/source/organizeImports` findings in your files matter.
- Never use built-in WebSearch or WebFetch (see CLAUDE.md). Use the MCP web-access tools if present, otherwise work from the repo.
- Any Tauri command taking a `WorkspaceEnv` must be listed in `src-tauri/src/modules/ssh_commands_guard_tests.rs`, with a remote test (see `remote/live_server_tests.rs` for the pattern). A command that does nothing on a server can take a plain parameter instead, as `agent_command_available(distro: Option<String>)` does.
- Do not remove or weaken anything that uses askpass or passphrases in the SSH code.

## Editing pitfalls seen in this repo
- If you edit files with Python, read and write bytes. Keep CRLF (`nl = "\r\n" if "\r\n" in raw else "\n"`) and use `encoding="utf-8"`; cp1252 corrupted characters before. Assert that each replaced pattern occurs exactly once.
- Do not write regexes or escapes (`\b`, `\n`, `\u0000`) through a bash heredoc into Python. They turned into real control bytes twice. Use the Edit tool for those lines.
- `pnpm exec tsc --noEmit -p .` takes a few minutes. The full `pnpm exec vitest run` takes about 10 minutes. `cargo test --lib` (from `src-tauri/`) takes about 5 minutes once built.
- **Never run the full cargo suite and the full vitest suite at the same time.** Under that load, about 11 Rust tests and `src/app/dist-budget.test.ts` fail spuriously; they all pass when run alone. Known flakies, which pass alone: `dist-budget`, `AdaptedGhosttyGpu`, and the git tests `the_upstream_is_the_ref_the_fetch_refspec_maps_not_origin_slash_branch` and `a_new_checkpoint_replaces_the_restore_as_the_baseline`.
- Render tests use `renderToStaticMarkup`, where zustand stores show their initial state. Test presentational components with props instead.

## Code worth reading first
- Rust PTY:
  - `src-tauri/src/modules/pty/` (`session.rs` has `Session::replay`; `replay.rs`, `hub.rs`, `wire.rs`, `protocol.rs`, `attachment.rs`, `da_filter.rs`, `remote.rs`);
  - `lib.rs` (command registration, managed state, window labels, `open_space_window`).
- HTTP server pattern: `src-tauri/src/modules/live_server/` (hyper 1, Host check, path confinement, `Cache-Control`, `nosniff`). Reuse its style.
- Frontend:
  - `src/modules/terminal/lib/pty-bridge.ts`;
  - `src/modules/terminal/ghostty/` (`useGhosttyTerminalSession.ts`, `AdaptedGhosttyTerminalModel.ts`, `GhosttyBlocks.ts`);
  - `src/modules/tabs/lib/useTabs.ts`;
  - `src/modules/tabs/lib/tabAgentStatus.ts`;
  - `src/modules/agents/lib/agentIcon.tsx`;
  - `src/modules/workspace/env.ts`;
  - `vite.config.ts` (the multi-page `rollupOptions.input`, around line 189).
- Settings UI conventions: `src/settings/` (`SettingsApp.tsx` TABS and VALID_TABS, `openSettingsWindow.ts` SettingsTab union, the sections).

## Test host (Phase 4)
- `aspirexc`: Ubuntu 26.04 x86_64, headless (Xvfb present, no desktop), 12 cores, 14 GiB. Tailscale IP `100.84.56.62`. SSH alias **`dy-aspire`** (user `dy`, key auth, passwordless sudo).
- `libwebkit2gtk-4.1-0` is not installed.
- Orca already runs there on port 6768. Use another port (for example 7681) and do not touch Orca.
- Build there from `dydev` rather than through CI.
- Use `ssh -o BatchMode=yes` and never try password logins. A separate host, `dypod`, has fail2ban, so never test failed SSH logins against it.

## Reporting
Report to the user in **Indonesian**, concisely. At the end of each phase, say:
- what was done;
- how it was verified, with real test output (say if a step was skipped or something failed);
- what was not tested manually.

Keep the user's email address private; never send it to an external service.
