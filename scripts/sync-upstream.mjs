#!/usr/bin/env node
// Merge upstream/main into this fork's main through a PR from sync/upstream:
//
//   node scripts/sync-upstream.mjs              merge; stops on conflicts
//   node scripts/sync-upstream.mjs --continue   commit the resolved merge
//   add --push to push sync/upstream and open (or reuse) the PR into main
//
// See docs/contributing/upstream-sync.md.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";

const UPSTREAM_URL = "https://github.com/crynta/terax-ai.git";
const UPSTREAM_REMOTE = "upstream";
const UPSTREAM_BRANCH = "main";
const BASE_BRANCH = "main";
const SYNC_BRANCH = "sync/upstream";

const args = new Set(process.argv.slice(2));
const unknown = [...args].filter((a) => !["--continue", "--push", "--help"].includes(a));
if (args.has("--help") || unknown.length > 0) {
  if (unknown.length > 0) console.error(`Unknown option: ${unknown.join(" ")}`);
  console.log("Usage: node scripts/sync-upstream.mjs [--continue] [--push]");
  process.exit(unknown.length > 0 ? 2 : 0);
}

const git = (...a) => execFileSync("git", a, { encoding: "utf8" }).trim();
const gitTry = (...a) => spawnSync("git", a, { encoding: "utf8" });
const run = (cmd, a) => {
  const r = spawnSync(cmd, a, { stdio: "inherit" });
  if (r.status !== 0) fail(`${cmd} ${a.join(" ")} failed`);
};
function fail(msg) {
  console.error(`\n✖ ${msg}`);
  process.exit(1);
}

const upstreamRef = `${UPSTREAM_REMOTE}/${UPSTREAM_BRANCH}`;
const baseRef = `origin/${BASE_BRANCH}`;
const gitDir = git("rev-parse", "--git-dir");
const merging = existsSync(join(gitDir, "MERGE_HEAD"));
const unmerged = () => git("diff", "--name-only", "--diff-filter=U").split("\n").filter(Boolean);

if (args.has("--continue")) {
  if (git("branch", "--show-current") !== SYNC_BRANCH) fail(`Not on ${SYNC_BRANCH}.`);
  if (merging) {
    const left = unmerged();
    if (left.length > 0) fail(`Still unresolved:\n  ${left.join("\n  ")}\nFix them and git add, then rerun.`);
    run("git", ["commit", "--no-edit"]);
  }
} else {
  if (merging) fail("A merge is already in progress. Resolve it and run with --continue.");
  if (git("status", "--porcelain") !== "") fail("Working tree is not clean. Commit or stash first.");

  if (gitTry("remote", "get-url", UPSTREAM_REMOTE).status !== 0) {
    run("git", ["remote", "add", UPSTREAM_REMOTE, UPSTREAM_URL]);
  }
  run("git", ["fetch", "--no-tags", "origin", BASE_BRANCH]);
  run("git", ["fetch", "--no-tags", UPSTREAM_REMOTE, UPSTREAM_BRANCH]);

  const behind = Number(git("rev-list", "--count", `${baseRef}..${upstreamRef}`));
  if (behind === 0) {
    console.log(`✔ ${BASE_BRANCH} already contains ${upstreamRef}.`);
    process.exit(0);
  }
  console.log(`\n${BASE_BRANCH} is ${behind} commit(s) behind ${upstreamRef}:`);
  console.log(git("log", "--no-merges", "--oneline", `${baseRef}..${upstreamRef}`));

  run("git", ["switch", "-C", SYNC_BRANCH, baseRef]);
  const merge = gitTry(
    "merge", "--no-ff", "--no-edit",
    "-m", `Merge ${upstreamRef} into ${BASE_BRANCH} (${behind} commits)`,
    upstreamRef,
  );
  if (merge.status !== 0) {
    const files = unmerged();
    if (files.length === 0) fail(merge.stderr || "git merge failed");
    console.log(`\nConflicts in ${files.length} file(s):\n  ${files.join("\n  ")}`);
    console.log("\nResolve them, git add each file, then:");
    console.log("  node scripts/sync-upstream.mjs --continue --push");
    process.exit(1);
  }
}

console.log(`\n✔ ${SYNC_BRANCH} holds the merge. Run the checks before pushing:`);
console.log("  pnpm lint && pnpm check-types && pnpm test");

if (!args.has("--push")) {
  console.log("Then: node scripts/sync-upstream.mjs --continue --push");
  process.exit(0);
}

run("git", ["push", "--force-with-lease", "origin", `HEAD:refs/heads/${SYNC_BRANCH}`]);
const existing = execFileSync(
  "gh",
  ["pr", "list", "--head", SYNC_BRANCH, "--base", BASE_BRANCH, "--state", "open", "--json", "url", "--jq", ".[0].url // empty"],
  { encoding: "utf8" },
).trim();
if (existing) {
  console.log(`✔ PR updated: ${existing}`);
} else {
  run("gh", [
    "pr", "create", "--base", BASE_BRANCH, "--head", SYNC_BRANCH,
    "--title", `Sync crynta/terax-ai@${UPSTREAM_BRANCH}`,
    "--body", "Manual upstream merge (conflicts resolved locally). Merge with **Create a merge commit**, not squash.",
  ]);
}
