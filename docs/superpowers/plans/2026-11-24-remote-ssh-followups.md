# Plan: Remote SSH, follow-ups before 0.9.5-dy

The limits Phase 6 left (see its plan), done before the walkthrough. Branch `dydev`, each task verified, committed and pushed on its own.

### Task 1: Background processes on a server

- [x] `shell_bg_spawn` in a server's space runs `sh -c` in a terminal of its own through the protocol's PTY, which streams output (a one-shot `exec` only returns it whole). Logs, kill and list work as locally; a dropped connection ends it with the reason in its log.
- [x] The agent on Windows (tests only) finds a bare program on `PATH`, which ConPTY does not.

### Task 2: A disconnected terminal comes back by itself

- [x] A server's terminal that ended because the connection dropped starts again in the same pane when that server is ready again; Retry stays for a failed reconnect.

Done. `onNextReady(host, then)` waits for a later ready epoch than the current one (the drop can arrive before the state says so). `reopenWhenBack` respawns in the old terminal's folder only while that server's space is active, because a new terminal takes the active workspace; elsewhere Retry stays. Cancelled on respawn, dispose and handoff.

### Task 3: New files on a server reach the file finder

- [ ] The finder's index of a server's folder is refreshed when the finder opens and the index is older than a short interval, so files made on the server in folders nobody watches show up.

### Task 4: A server's space in the tab strip

- [ ] The tab strip shows the host of a server's space.

### Task 5: On a real server, docs

- [ ] dypod e2e: a background process (output while running, exit code, kill); TERAX.md; plan marked complete.
