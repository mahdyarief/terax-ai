---
title: "Upstream sync is manual: one sync/upstream PR into main, merged with a merge commit, then dydev follows main"
author: Mahdy
date: 2026-11-22
branch: dydev
cost: { minutes: 1, tokens: 1645 }
id: l-e37e888b
scope: repo
status: active
src: agent
touches:
  - src/modules/ai/config.ts
---

## Decided
- The owner syncs upstream (crynta/terax-ai) by hand; no scheduled workflow. Upstream reaches `main` only through one PR from `sync/upstream`, cut fresh from `main` and merged with `upstream/main` by `scripts/sync-upstream.mjs`. After it lands, `dydev` is brought up to `main` (fast-forward when it has no commits of its own).
- Sync PRs are always merged with a merge commit, never squashed: a squash drops upstream history, so the next sync replays the same commits and conflicts on them.

## Tried and ruled out
- A daily GitHub Actions sync (auto PR, conflict issue, auto fast-forward of dydev): the owner chose to do it manually. It also needed a PAT secret, because this repo has "Allow GitHub Actions to create and approve pull requests" off, GITHUB_TOKEN PRs start no CI, and pushing upstream `.github/workflows` changes needs Workflows write.

## Watch out
- Merging upstream into the fork really conflicts in fork-modified files (e.g. `src/modules/ai/config.ts`, markdown preview, `App.tsx`); keep the fork's release bits (`v*-dy` exclusion in release.yml, release-dy.yml).

In the code:
- `scripts/sync-upstream.mjs` (`--continue`, `--push`), `docs/contributing/upstream-sync.md`
