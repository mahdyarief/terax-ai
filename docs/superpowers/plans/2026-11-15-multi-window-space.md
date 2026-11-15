# Multi-Window Space Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable opening an existing space in a separate Tauri window while keeping space/tab state synchronized across windows and preserving per-window PTY ownership.

**Architecture:** Keep a single persisted store (`terax-spaces.json`) for space metadata and per-space tab state, extended with `hostWindowLabel` and per-window active-space tracking. Each window boots and renders only tabs for spaces it hosts; PTYs remain owned by the host webview label (`pty_open` in `src-tauri/src/modules/pty/mod.rs`). Initial handoff respawns PTYs in the target window (no live PTY transfer). Cross-window consistency is achieved via scoped persistence and Tauri events.

**Tech Stack:** React + TypeScript (Zustand, Vite), Tauri v2 (Rust commands, capabilities, window events), `@tauri-apps/plugin-store` (`LazyStore`), Vitest.

**Tracking:** `docs/multi-window-space-plan.md` holds the high-level context, risks, and progress checklist. Update both this plan and that checklist as tasks complete.

---

### Task 1: Add space hosting metadata and migration

**Files:**
- Modify: `src/modules/spaces/lib/store.ts`
- Create: `src/modules/spaces/lib/store.test.ts`

- [x] **Step 1: Write the failing test**

```ts
import { describe, expect, it } from "vitest";
import { migrateSpaceMeta } from "./store";

describe("migrateSpaceMeta", () => {
  it("defaults missing hostWindowLabel to main", () => {
    const migrated = migrateSpaceMeta({
      id: "default",
      name: "Default",
      root: null,
      env: "host",
      createdAt: 1,
      updatedAt: 1,
    } as any);
    expect(migrated.hostWindowLabel).toBe("main");
  });

  it("preserves explicit hostWindowLabel", () => {
    const migrated = migrateSpaceMeta({
      id: "s1",
      name: "Space 1",
      root: null,
      env: "host",
      createdAt: 1,
      updatedAt: 1,
      hostWindowLabel: "space-s1",
    } as any);
    expect(migrated.hostWindowLabel).toBe("space-s1");
  });
});
```

- [x] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/modules/spaces/lib/store.test.ts`
Expected: FAIL (missing export `migrateSpaceMeta` / missing field behavior).

- [x] **Step 3: Write minimal implementation**

```ts
export type SpaceMeta = {
  id: string;
  name: string;
  root: string | null;
  env: WorkspaceEnv;
  color?: number;
  /** Webview label hosting this space. undefined/null => "main" for compat. */
  hostWindowLabel?: string | null;
  createdAt: number;
  updatedAt: number;
};

export function migrateSpaceMeta(space: SpaceMeta): SpaceMeta {
  return {
    ...space,
    hostWindowLabel: space.hostWindowLabel ?? "main",
  };
}
```

Apply migration in `loadAll()` when reading `KEY_SPACES`:
```ts
if (k === KEY_SPACES) spaces = ((v as SpaceMeta[]) ?? []).map(migrateSpaceMeta);
```

- [x] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/modules/spaces/lib/store.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/modules/spaces/lib/store.ts src/modules/spaces/lib/store.test.ts
git commit -m "feat(spaces): add hostWindowLabel metadata and migration"
```

### Task 2: Per-window active space persistence (scoped, non-destructive)

**Files:**
- Modify: `src/modules/spaces/lib/store.ts`
- Modify: `src/modules/spaces/lib/store.test.ts`

- [x] **Step 1: Write the failing test**

```ts
import { getActiveSpaceIdForWindow, setActiveSpaceIdForWindowMap } from "./store";

it("reads active space per window with fallback", () => {
  const map = { main: "default", "space-a": "a" };
  expect(getActiveSpaceIdForWindow(map, "space-a")).toBe("a");
  expect(getActiveSpaceIdForWindow(map, "space-missing")).toBeNull();
  expect(getActiveSpaceIdForWindow(null, "main")).toBeNull();
});
```

- [x] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/modules/spaces/lib/store.test.ts`
Expected: FAIL (helpers missing).

- [x] **Step 3: Write minimal implementation**

Add key and helpers (keep legacy `KEY_ACTIVE` readable for migration, but prefer per-window map):

```ts
const KEY_ACTIVE_BY_WINDOW = "activeIdByWindow";

export type ActiveSpaceByWindow = Record<string, string | null>;

export function getActiveSpaceIdForWindow(
  map: ActiveSpaceByWindow | null | undefined,
  windowLabel: string,
): string | null {
  return map?.[windowLabel] ?? null;
}
```

Extend `LoadedSpaces` to include `activeIdByWindow` and load both keys. Provide `saveActiveIdForWindow(windowLabel, id)` that updates only that entry to reduce cross-window clobbering (read-modify-write still needs coordination; document/guard via `updatedAt` where needed).

- [x] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/modules/spaces/lib/store.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/modules/spaces/lib/store.ts src/modules/spaces/lib/store.test.ts
git commit -m "feat(spaces): persist active space per window"
```
### Task 3: Refactor `useSpaces` for hosted spaces and window-scoped active state

**Files:**
- Modify: `src/modules/spaces/lib/useSpaces.ts`
- Create: `src/modules/spaces/lib/useSpaces.hosting.test.ts`

- [x] **Step 1: Write the failing test**

```ts
import { describe, expect, it } from "vitest";
import { selectHostedSpaces } from "./useSpaces";

it("filters spaces hosted by current window", () => {
  const spaces = [
    { id: "a", hostWindowLabel: "main" },
    { id: "b", hostWindowLabel: "space-b" },
  ] as any;
  expect(selectHostedSpaces(spaces, "main").map((s) => s.id)).toEqual(["a"]);
});
```

- [x] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/modules/spaces/lib/useSpaces.hosting.test.ts`
Expected: FAIL.

- [x] **Step 3: Write minimal implementation**

Add/export pure selectors and store fields/actions:
- `windowLabel: string` (set on boot)
- `activeId` represents the active space for this window
- `selectHostedSpaces(spaces, windowLabel)` returns spaces whose `hostWindowLabel` (defaulted to `"main"`) equals the label
- `assignSpaceToWindow(spaceId, hostWindowLabel)` updates metadata immutably and bumps `updatedAt`
- `hydrateForWindow({ spaces, activeIdForWindow, windowLabel })`

Keep existing actions (`create/rename/reorder/setEnv/setColor/delete`) working; ensure they preserve `hostWindowLabel`.

- [x] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/modules/spaces/lib/useSpaces.hosting.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/modules/spaces/lib/useSpaces.ts src/modules/spaces/lib/useSpaces.hosting.test.ts
git commit -m "refactor(spaces): scope space store to host window"
```

### Task 4: Refactor `useTabs` for detach/attach and hosted-space guards

**Files:**
- Modify: `src/modules/tabs/lib/useTabs.ts`
- Create: `src/modules/tabs/lib/useTabs.transfer.test.ts`

- [x] **Step 1: Write the failing test**

```ts
import { describe, expect, it } from "vitest";
import { detachTabsBySpace } from "./useTabs";

it("detaches all tabs for a space and keeps remaining", () => {
  const tabs = [
    { id: 1, spaceId: "a" },
    { id: 2, spaceId: "b" },
  ] as any;
  const { detached, remaining } = detachTabsBySpace(tabs, "a");
  expect(detached.map((t) => t.id)).toEqual([1]);
  expect(remaining.map((t) => t.id)).toEqual([2]);
});
```

- [x] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/modules/tabs/lib/useTabs.transfer.test.ts`
Expected: FAIL.

- [x] **Step 3: Write minimal implementation**

Export pure helpers and wire store actions:
- `detachTabsForSpace(spaceId)` returns `{ detached, remaining, nextActiveId }` and updates store without disposing PTYs (caller decides lifecycle)
- `attachTabsForSpace(spaceId, tabs, desiredActiveId?)`
- Guard mutating actions so a window cannot create/move/activate tabs into a non-hosted space (except explicit transfer paths)

Preserve existing single-window behavior (`main` hosts current spaces).

- [x] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/modules/tabs/lib/useTabs.transfer.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/modules/tabs/lib/useTabs.ts src/modules/tabs/lib/useTabs.transfer.test.ts
git commit -m "refactor(tabs): support detach/attach by space for multi-window"
```
### Task 5: Scope boot and persistence to the current window

**Files:**
- Modify: `src/modules/spaces/lib/useSpacesBoot.ts`
- Modify: `src/modules/spaces/lib/useSpacePersistence.ts`
- Modify: `src/main.tsx`
- Modify: `src/app/App.tsx`
- Create: `src/modules/spaces/lib/useSpacesBoot.test.ts`

- [x] **Step 1: Write the failing test**

```ts
import { describe, expect, it } from "vitest";
import { spacesForWindow } from "./useSpacesBoot";

it("boot only hydrates spaces hosted by this window", () => {
  const spaces = [
    { id: "a", hostWindowLabel: "main" },
    { id: "b", hostWindowLabel: "space-b" },
  ];
  const hosted = spacesForWindow(spaces as any, "space-b");
  expect(hosted.map((s) => s.id)).toEqual(["b"]);
});
```

- [x] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/modules/spaces/lib/useSpacesBoot.test.ts`
Expected: FAIL.

- [x] **Step 3: Write minimal implementation**

- Resolve `windowLabel` via `getCurrentWindow().label` in `src/main.tsx`/`App.tsx` and pass down.
- In `useSpacesBoot`: load all spaces, migrate, hydrate only hosted subset for `windowLabel`, restore tabs only for those spaces, and pick the active tab inside the hosted active space.
- In `useSpacePersistence`: persist tab state **only** for `hostedSpaceIds`; never write state keys for spaces hosted elsewhere. Keep the debounce/visibility/beforeunload flush and expose `flushNow()` for handoff.
- In `src/main.tsx`: stop calling the global `pty_close_all()` unconditionally on startup; it kills PTYs owned by other windows. Use a per-label reset (e.g. `reset_webview(currentLabel)` / a new `pty_close_all_for_label` command) once available, or gate the call until backend support lands.

- [x] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/modules/spaces/lib/useSpacesBoot.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/modules/spaces/lib/useSpacesBoot.ts src/modules/spaces/lib/useSpacePersistence.ts src/main.tsx src/app/App.tsx src/modules/spaces/lib/useSpacesBoot.test.ts
git commit -m "refactor(spaces): scope boot and persistence to host window"
```

### Task 6: Backend window command, capability, and per-label cleanup

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/src/modules/pty/mod.rs` (add per-label close/reset if needed; do not change owner semantics yet)
- Modify: `src-tauri/tauri.conf.json` only if build inputs require it

- [ ] **Step 1: Write the failing check**

Add a Rust test for the label-to-cleanup mapping helper (e.g. `space_window_label("abc") == "space-abc"` and parsing back), so window identity logic is covered without a GUI.

```rust
#[test]
fn space_window_label_round_trips() {
    assert_eq!(space_window_label("abc"), "space-abc");
    assert_eq!(space_id_from_label("space-abc"), Some("abc".to_string()));
    assert_eq!(space_id_from_label("main"), None);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml space_window_label`
Expected: FAIL (missing functions).

- [x] **Step 3: Write minimal implementation**

```rust
fn space_window_label(space_id: &str) -> String {
    format!("space-{space_id}")
}

fn space_id_from_label(label: &str) -> Option<String> {
    label.strip_prefix("space-").map(|s| s.to_string())
}
```

Then add the command (idempotent):

```rust
#[tauri::command]
async fn open_space_window(app: tauri::AppHandle, space_id: String) -> Result<(), String> {
    let label = space_window_label(&space_id);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    let url = format!("index.html?windowKind=space&spaceId={space_id}");
    let builder = tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App(url.into()))
        .title("Terax")
        .inner_size(1100.0, 760.0)
        .min_inner_size(720.0, 480.0)
        .visible(true);
    builder.build().map_err(|e| e.to_string())?;
    Ok(())
}
```

Register `open_space_window` in `invoke_handler`. Match the main window's decoration/transparency/titlebar config from `tauri.conf.json` so the new window looks identical.

In the window-event setup, handle `Destroyed` for each app window by invoking the existing per-label cleanup path (`pty` `destroy_webview(label)`) and, for `space-*` labels, emitting `spaces:host-changed` so the main window can reclaim the space.

- [ ] **Step 4: Run checks**

Run: `cargo test --manifest-path src-tauri/Cargo.toml space_window_label`
Run: `pnpm check-types`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/capabilities/default.json src-tauri/src/modules/pty/mod.rs src-tauri/tauri.conf.json
git commit -m "feat(tauri): open and clean up per-space windows"
```### Task 7: UI "Open in new window" and handoff flow

**Files:**
- Modify: `src/modules/spaces/SpaceSwitcher.tsx`
- Modify: `src/app/App.tsx`
- Modify: `src/modules/spaces/lib/useSpacePersistence.ts` (ensure `flushNow` exists)
- Create: `src/modules/spaces/lib/handoff.test.ts`

- [x] **Step 1: Write the failing test**

```ts
import { describe, expect, it } from "vitest";
import { planSpaceHandoff } from "./handoff";

it("focuses an existing window instead of re-hosting", () => {
  expect(planSpaceHandoff({ spaceId: "a", hostWindowLabel: "space-a" }, "main"))
    .toEqual({ kind: "focus", windowLabel: "space-a" });
});

it("hands off a locally hosted space", () => {
  expect(planSpaceHandoff({ spaceId: "a", hostWindowLabel: "main" }, "main"))
    .toEqual({ kind: "handoff", spaceId: "a", from: "main", to: "space-a" });
});
```

- [x] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/modules/spaces/lib/handoff.test.ts`
Expected: FAIL.

- [x] **Step 3: Write minimal implementation**

```ts
export type HandoffPlan =
  | { kind: "focus"; windowLabel: string }
  | { kind: "handoff"; spaceId: string; from: string; to: string };

export function planSpaceHandoff(
  space: { spaceId: string; hostWindowLabel: string | null | undefined },
  currentWindowLabel: string,
): HandoffPlan {
  const host = space.hostWindowLabel ?? "main";
  if (host !== currentWindowLabel) return { kind: "focus", windowLabel: host };
  return {
    kind: "handoff",
    spaceId: space.spaceId,
    from: currentWindowLabel,
    to: `space-${space.spaceId}`,
  };
}
```

Wire the UI:
- `SpaceSwitcher` gains `onOpenSpaceInWindow(spaceId)`; render a per-space action labelled "Open in new window", or "Focus window" when hosted elsewhere.
- `App.tsx` `handleOpenSpaceInWindow(spaceId)`:
  1. warn/confirm if any tab in the space has a foreground job (best-effort),
  2. `await flushNow()`,
  3. close/reset PTYs for tabs in that space in this window (respawn mode),
  4. `detachTabsForSpace(spaceId)` and choose the next active tab/space,
  5. `assignSpaceToWindow(spaceId, targetLabel)`,
  6. `invoke("open_space_window", { spaceId })`.

- [x] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/modules/spaces/lib/handoff.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/modules/spaces/SpaceSwitcher.tsx src/app/App.tsx src/modules/spaces/lib/useSpacePersistence.ts src/modules/spaces/lib/handoff.ts src/modules/spaces/lib/handoff.test.ts
git commit -m "feat(spaces): add open space in new window flow"
```

### Task 8: Cross-window sync, delete/reorder/close semantics

**Files:**
- Modify: `src/modules/spaces/lib/useSpaces.ts`
- Modify: `src/modules/spaces/lib/useSpacePersistence.ts`
- Modify: `src/app/App.tsx`
- Modify: `src-tauri/src/lib.rs`
- Create: `src/modules/spaces/lib/sync.test.ts`

- [x] **Step 1: Write the failing test**

```ts
import { describe, expect, it } from "vitest";
import { mergeSpacesByUpdatedAt, applyDeletedSpace } from "./sync";

it("keeps the newer metadata per space id", () => {
  const local = [{ id: "a", name: "Old", updatedAt: 1 }];
  const remote = [{ id: "a", name: "New", updatedAt: 2 }];
  expect(mergeSpacesByUpdatedAt(local, remote)[0].name).toBe("New");
});

it("removes deleted spaces", () => {
  const spaces = [{ id: "a" }, { id: "b" }];
  expect(applyDeletedSpace(spaces, "a").map((s) => s.id)).toEqual(["b"]);
});
```

- [x] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/modules/spaces/lib/sync.test.ts`
Expected: FAIL.

- [x] **Step 3: Write minimal implementation**

- Emit/listen to `spaces:meta-changed`, `spaces:host-changed`, `spaces:deleted`, `spaces:reordered`; merge by id with `updatedAt` last-write-wins.
- Delete: only the hosting window performs it; close PTYs/tabs first, persist deletion, broadcast, and have other windows drop the space from lists. A space window showing a deleted space closes or falls back.
- Close: on `space-*` `Destroyed`, reclaim `hostWindowLabel` to `"main"`, persist + broadcast, and ensure `destroy_webview(label)` cleanup ran. Define main-window close behavior (close all children with cleanup, or confirm).
- Move tab to a space hosted in another window: block in v1 with a clear message.
- Audit hardcoded `"main"` in `src-tauri/src/modules/control.rs` and `agent_events.rs`; make window-aware or document the fallback.

- [ ] **Step 4: Run tests and verify**

Run: `pnpm test` and `pnpm check-types`.
Expected: PASS. Manual: two windows open; rename/reorder/delete/host changes sync; closing a space window reclaims without orphan PTYs.

- [ ] **Step 5: Commit**

```bash
git add src/modules/spaces src/app/App.tsx src-tauri/src/lib.rs
git commit -m "feat(spaces): synchronize multi-window space state and lifecycle"
```

## Final verification checklist

- [ ] Single-window behavior on `"main"` is unchanged.
- [ ] "Open in new window" moves hosting and respawns terminals in the target window.
- [ ] Startup no longer kills PTYs owned by other windows.
- [ ] Rename/color/env/reorder/delete/host changes sync across windows.
- [ ] Closing a space window reclaims/cleans up with no orphan sessions.
- [ ] `pnpm check-types`, relevant Vitest suites, and Rust checks pass.
- [ ] `docs/multi-window-space-plan.md` checklist updated.
