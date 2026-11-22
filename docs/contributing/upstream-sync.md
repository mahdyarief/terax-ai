# Syncing with upstream

This fork (`mahdyarief/terax-ai`) follows `crynta/terax-ai`. Syncing is done by hand. Upstream commits reach `main` through one PR, never by pushing to `main` directly.

```
upstream/main ──merge──▶ sync/upstream ──PR + CI──▶ main ──fast-forward──▶ dydev
```

| Branch | Role |
|---|---|
| `main` | Released fork code. Only changes through PRs. |
| `sync/upstream` | Holds only the upstream merge. Rebuilt from `main` on each sync. Do not put your own work on it. |
| `dydev` | Working branch. Brought up to `main` after each sync. |

## Steps

**1. Merge upstream into `sync/upstream`.** The script fetches both remotes, shows the commits `main` is missing, and stops on conflicts. If `main` is already up to date, it says so and stops.

```sh
node scripts/sync-upstream.mjs
```

**2. If there are conflicts, resolve them,** run `git add` on each file, then run the checks:

```sh
pnpm lint && pnpm check-types && pnpm test
```

**3. Push and open the PR.** If the merge was committed in step 1 (no conflicts), this just pushes. If an open PR already exists, the script reuses it.

```sh
node scripts/sync-upstream.mjs --continue --push
```

**4. Merge the PR once CI passes, with Create a merge commit.** Do not squash. A squash drops upstream's history, so the next sync would treat the same commits as new and conflict on them.

```sh
gh pr merge <number> --merge --delete-branch
```

**5. Bring `dydev` up to `main`:**

```sh
git switch dydev
git pull --ff-only origin main   # if dydev has its own commits: git merge origin/main
git push origin dydev
```

## When resolving conflicts, keep the fork's changes

- `release.yml` keeps skipping `v*-dy` tags, and `release-dy.yml` stays.
- The dy release tagging (`v<version>-dy`).
- The app updater still points at upstream's releases. If an upstream release would overwrite dy installs, deal with it in the sync PR.
