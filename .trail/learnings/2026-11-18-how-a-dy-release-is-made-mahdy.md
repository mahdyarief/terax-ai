---
title: "How a dy release is made: tag v<version>-dy, Windows built locally, macOS and Linux on GitHub"
author: Mahdy
date: 2026-11-18
branch: dydev
cost: { minutes: 29, tokens: 84162 }
id: l-e99348fb
scope: repo
status: active
src: agent
touches:
  - ".github/workflows/release-dy.yml"
  - ".github/workflows/release.yml"
  - ".size-limit.json"
  - CHANGELOG.md
  - src/modules/editor/TextHeadPreview.tsx
  - src-tauri/tauri.nosign.conf.json
---

## Decided
- A dy release is tagged `v<version>-dy` (the owner's choice over `dy-v<version>`). `release.yml` excludes `v*-dy`, because it needs upstream's signing secrets and would fail on this fork; `release-dy.yml` handles those tags instead.
- Windows installers are built on the owner's machine with `pnpm build:nosign` and uploaded with `gh release upload`. macOS and Linux are built by `release-dy.yml`, because this machine has no macOS, no WSL distribution and no Docker.
- All dy installers are unsigned. The macOS build is ad-hoc signed (`signingIdentity: "-"`), since an Apple Silicon Mac will not start an app with no signature at all.
- The total client JS budget in `.size-limit.json` is 3150 KB: Mermaid adds about 1.5 MB gzipped, loaded only for documents with diagrams.

## Watch out
- A version bump changes `package.json`, `src-tauri/tauri.conf.json`, the `[package]` version in `src-tauri/Cargo.toml`, `src-tauri/tauri.windows.conf.json` (wix, no suffix) and, since the remote agent exists, `src-tauri/crates/terax-agent/Cargo.toml` too: the app reinstalls an agent whose version differs from its own, and `the_agent_carries_the_apps_version` fails until the two match. `[workspace.package]` is the CLI's version and stays as it is. The SSH feature ships as 0.9.5-dy (the owner's plan, 2026-11-23); until then dydev carries 0.9.4-dy.
- Since the remote agent is bundled, `build:nosign` first runs `pnpm build:agent`, which needs zig (`pip install ziglang`), `cargo-zigbuild` and the two musl Rust targets; `tauri.nosign.conf.json` lists the agents as resources, so any build with that config fails without them. `release-dy.yml` passes that config too, so its macOS and Linux jobs need a `pnpm build:agent` step before the 0.9.5-dy release.
- A release build of the Rust side takes about 30 minutes here. The release and the tag can be created first; the tag starts the macOS and Linux builds while Windows still compiles.
- Publishing a release starts "Update Nix sources", which fails in seconds because it downloads assets from the upstream repo. It changes nothing.
- The app's updater still points at upstream's releases. When upstream ships 0.9.3 or later, a dy install will be offered that update and would lose the dy changes.

In the code:
- `.github/workflows/release-dy.yml`, `.github/workflows/release.yml` (tag filter), `src-tauri/tauri.nosign.conf.json`, `CHANGELOG.md`
- `gh release create v<version>-dy -R mahdyarief/terax-ai --target main`
