# Task: fix the full Terax app in the browser (web remote)

You are working in the Terax repo (Tauri 2: Rust in `src-tauri/`, React/TS in `src/`).
- Working copy: `D:\Github\agent-workspace\terax-ai`.
- Branch: **`dydev`**. Everything stays on this one branch; do not create others.

## Read first, in full
1. **`docs/superpowers/plans/2026-11-28-web-remote-app-fixes/plan.md`**: the plan for this task. It has root causes, decisions, Tasks 1–7, the order, verification notes and risks.
2. `docs/superpowers/plans/2026-11-28-web-remote-full-app/agent-prompt.md`: the rules that apply here too.
   - Security rules.
   - Hard rules: never stop or kill any `terax.exe` on this machine, and never run `pnpm dev:app` here without asking.
   - pnpm only, no `cargo fmt`, no `biome format --write` on existing files.
   - Test host and deploy steps.
   - Pitfalls.
3. `docs/superpowers/plans/2026-11-28-web-remote-full-app/plan.md`: how the bridge works (Status, Decisions, Phase 1–2).
4. The learnings in `.trail/learnings/` dated 2026-11-28. Four of them are about this bridge:
   - the browser never manages remote access, devices or proxy ports;
   - loopback is decided from the peer address;
   - an `AppHandle` field in test-built structs breaks the Windows test binary;
   - terminal stream requests must carry the device cookie.

## Before you start
- **Work in progress on Task 7:** `src-tauri/src/modules/web_remote/devices.rs` has three new tests that do not compile yet (they need `PAIRING_OPEN_CODES`). They are uncommitted.
  - Do not stash or discard them.
  - Start with Task 7 (or Task 1, then Task 7), finishing it from these tests.
- **Confirm two decisions with the user before Task 2**, and record the answers in the plan's Decisions:
  - Headless host (aspirexc): should the browser session take over the spaces hosted by `main` automatically (recommended), or always create its own space?
  - One app session per device: opening `/app/` in a second tab of the same device closes the first ("opened in another tab"). Is that OK? The mirror keeps working in parallel.
- Keep the order of tasks from the plan: **1 → 7 → 2 → 3 → 4 → 5 → 6**.

## How to work
- One task = one commit. Write the test first (RED, then GREEN).
- Tick the plan's checkboxes and update its **Status** line in the same commit.
- After each finished task, **commit and push to `dydev` without asking**.
- Commit messages end with:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
- Run `trail check` before each commit.
- When a task settles a non-obvious cause or decision, write a learning:
  - command: `trail note --title "<plain words>"`;
  - text on stdin, with `## Decided`, `## Tried and ruled out`, `## Watch out` and an `In the code:` line;
  - commit it with the change.
- Never merge to `main`, tag, release or open a PR. Do not trigger GitHub Actions.
- Ask before:
  - adding a dependency (enabling tokio's `signal` feature for Task 7 is fine);
  - changing a command's class in `policy.rs` to anything less strict;
  - weakening pairing, grants, Host/Origin checks, `X-Terax-Remote` or CSP.
  `workspace_pick_folder` stays `Native`.

## Deploy and verify (aspirexc)
- Deploy after Tasks 1+7, after Task 3, and after Task 4.
- Steps:
  1. `ssh -o BatchMode=yes dy-aspire`;
  2. `cd ~/terax-ai && git pull --ff-only`;
  3. `bash /tmp/terax-build.sh > ~/tmp/terax-build.log 2>&1`;
  4. `systemctl --user stop terax-remote && install -m755 src-tauri/target/release/terax ~/.local/bin/terax && systemctl --user start terax-remote`.
- **Restarting the service ends the terminals the user has open there. Tell the user first.**
- Never set `TERAX_REMOTE_SPIKE` on the service, and do not touch Orca (port 6768).
- The Lenovo laptop's Chrome is paired as "Lenovo laptop" with `input`, `tabs` and `app`. Use it through `fastmcp-browser` at `http://100.84.56.62:7681/app/`.
  - The plan's "Verification notes" list what that tool can and cannot do.
  - Evidence comes from the network log and from side effects on aspirexc.
  - Unit tests alone are not enough: the cookie bug passed every unit and live test.
- For an API test device: pair one from the stderr pairing URL (or `systemctl --user reload terax-remote` for a new one, which sends `SIGHUP` to terax), grant it in `remote-devices.json` while the service is stopped, then remove it and its cookie file afterwards.

## Tests
- `cargo test --lib` from `src-tauri/`: last run 1132 passed.
- `pnpm exec vitest run`: last full run 5089 passed.
- `pnpm exec tsc --noEmit -p .`: clean.
- **Never run the full cargo and full vitest suites at the same time.**
- Compare biome `lint/…` findings against HEAD; CRLF "format" findings are noise.

## Reporting
Report to the user in **Indonesian**, concisely, after each deploy and at the end:
- what was done;
- how it was verified, with real output (say what was skipped or failed);
- what could not be checked in the browser.

Keep the user's email private.
