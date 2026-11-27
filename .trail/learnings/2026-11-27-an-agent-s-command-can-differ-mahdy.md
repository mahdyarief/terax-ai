---
title: "An agent's command can differ per workspace, and a missing CLI is only a soft hint"
author: Mahdy
date: 2026-11-27
branch: dydev
cost: { minutes: 28, tokens: 171477 }
id: l-1f072b2a
scope: repo
status: active
src: agent
touches:
  - src-tauri/src/modules/agent_command.rs
  - src/modules/agents/lib/profiles.test.ts
  - src/modules/agents/lib/commandAvailability.ts
  - src/modules/agents/lib/profiles.ts
  - src/settings/sections/CliAgentsSection.tsx
---

## Decided
- An agent profile has `workspaceCommands`, keyed by workspace scope key (`local`, `wsl:<distro>`, `ssh:<host>`). The launcher uses the active workspace's entry, else the agent's command. Editing in the launcher writes to whichever one is in effect; Reset there first drops the workspace entry.
- The PATH check never blocks a launch, it only shows "not found". Locally it uses the LSP module's `resolve_binary` (login-shell PATH on macOS). On WSL it runs the user's login shell with `-lic` and `command -v`, so .bashrc PATH (nvm, ~/.local/bin) and aliases count; fish falls back to sh.
- SSH is never checked: the PATH is the server's, and a probe would mean another remote exec per keystroke.
- The command takes `distro: Option<String>`, not a `WorkspaceEnv`, on purpose: it does nothing on a server, so it stays out of the SSH command guard list.

## Tried and ruled out
- Checking locally for aliases: `which` cannot see shell aliases or functions, so a local alias like `cc` shows "not found". The wording says an alias still works.

## Watch out
- `command_program` returns None (unknown) for shell syntax: variables, `~`, relative paths, subshells, globs. Leading `VAR=x`, `env`, `exec` and `command` are skipped.
- Results are cached for 30s per (distro, command), so a CLI installed meanwhile shows up soon.
- Running cargo test and the full vitest suite at the same time made 11 Rust tests and dist-budget fail; both pass when run alone.

In the code: src-tauri/src/modules/agent_command.rs (`agent_command_available`), src/modules/agents/lib/commandAvailability.ts, `commandForWorkspace` and `setWorkspaceCommand` in src/modules/agents/lib/profiles.ts, `WorkspaceCommandsField` in src/settings/sections/CliAgentsSection.tsx.
