---
title: "The remote agent must hide its own stdio from the commands it runs, and must never append unresolved dots to a checked path"
author: Mahdy
date: 2026-11-23
branch: dydev
cost: { minutes: 1, tokens: 2428 }
id: l-3b558852
scope: repo
status: active
src: agent
---

## Decided
- On Windows the agent clears the inherit flag on its own stdin and stdout when it starts. A child started by `exec` otherwise inherits them, holds the protocol stream open after the agent dies, and the app only notices the agent is gone when that command ends (seen as a 4.8 s delay, the length of the command). Unix needs nothing: a child's descriptors 0 to 2 are replaced and the rest close on exec.
- The agent's root check lets the operating system resolve `.` and `..` inside the part of a path that exists (the operation resolves them the same way), and refuses them in the part that does not exist yet or as the entry's own name.

## Tried and ruled out
- Refusing every `..`: the workspace contract needs `canonicalize("dir/..")` to work.
- Appending the unresolved names to the canonical parent and comparing text: `root/..` then reads as inside `root`, while `remove_dir_all` on it would remove the root's parent.

## Watch out
- Windows resolves `..` by the text, even past names that do not exist, so the same path can be `Outside` on Windows and `Malformed` on Linux. Both are refusals; tests should not insist on one.
- A walk cancelled before it starts is answered by the client without a round trip; sent to the agent, a small search finishes before the `Cancel` arrives.

In the code: `crates/terax-agent/src/main.rs` (`keep_stdio_from_children`), `crates/terax-agent/src/roots.rs` (`resolve`), `crates/terax-remote-protocol/src/client.rs` (`grep`, `list_files`); the tests are `crates/terax-agent/tests/contract.rs` (`killing_the_agent_mid_call_fails_the_call_promptly`) and `tests/serve.rs` (`an_entry_named_dot_dot_cannot_reach_the_roots_parent`).
