# Plan: Remote SSH, follow-ups before 0.9.5-dy

The limits Phase 6 left (see its plan), done before the walkthrough. Branch `dydev`, each task verified, committed and pushed on its own.

### Task 1: Background processes on a server

- [x] `shell_bg_spawn` in a server's space runs `sh -c` in a terminal of its own through the protocol's PTY, which streams output (a one-shot `exec` only returns it whole). Logs, kill and list work as locally; a dropped connection ends it with the reason in its log.
- [x] The agent on Windows (tests only) finds a bare program on `PATH`, which ConPTY does not.

### Task 2: A disconnected terminal comes back by itself

- [x] A server's terminal that ended because the connection dropped starts again in the same pane when that server is ready again; Retry stays for a failed reconnect.

Done. `onNextReady(host, then)` waits for a later ready epoch than the current one (the drop can arrive before the state says so). `reopenWhenBack` respawns in the old terminal's folder only while that server's space is active, because a new terminal takes the active workspace; elsewhere Retry stays. Cancelled on respawn, dispose and handoff.

### Task 3: New files on a server reach the file finder

- [x] The index already expired after 45 s everywhere (a folder nobody watches is the same locally); a server's now lives 10 s (`SERVER_TTL`), since only the explorer's open folders are watched there and the agent walks the tree in one call. The AI's `@` file list follows (10 s for a server, 60 s locally).

### Task 4: A server's space in the tab strip

- [x] The space switcher's trigger, where the tab strip starts, carries a server icon and names the host in its title ("<space> on <host>"), since the name is truncated. Tabs themselves have no per-tab env: every tab of a space is in that space's workspace.

### Task 5: On a real server, docs

- [ ] dypod e2e: a background process (output while running, exit code, kill); TERAX.md; plan marked complete.
