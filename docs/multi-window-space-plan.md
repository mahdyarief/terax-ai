# Rencana: Multi-Window Space ("Open in New Window")

## Ringkasan

- "New space": tetap membuat space baru di window saat ini.
- "Open in new window": membuka **space yang sudah ada** di window terpisah, bukan membuat space baru. Aksi ini memindahkan/menetapkan *hosting* space ke window target.
- Prinsip desain: **1 space di-host oleh maksimal 1 window pada satu waktu**. Tab/PTY yang dimiliki space mengikuti host window-nya, sedangkan persistensi state (spaces/tabs) tetap single-store dengan sinkronisasi event antar window.

## 1. Model data: kepemilikan space dan binding terminal/window

### 1.1 Metadata space (persisten, `src/modules/spaces/lib/store.ts`)

Tambahkan field hosting pada `SpaceMeta` (+ migrasi data lama):

- `hostWindowLabel?: string | null`
- Label webview yang saat ini meng-host space (`"main"` untuk utama; `"space-<spaceId>"` untuk window space; `null/undefined` ⇒ `"main"` backward compat).
- (opsional) `lastHostWindowLabel?: string`.

Aturan:

- `activeId` (space aktif) **per window**, bukan global tunggal.
- Persist global: daftar space + `hostWindowLabel` per space.
- Persist per-window: `activeSpaceIdByWindow[windowLabel]`.
- `DEFAULT_SPACE_ID` tetap valid, default host `"main"`.

### 1.2 State runtime frontend (Zustand, `src/modules/spaces/lib/useSpaces.ts`)

Refactor agar sadar multi-window:

- Tambah `hostedSpaceIds: string[]` dan/atau selector `spacesHostedIn(windowLabel)`.
- `activeId` = active space di window ini, bukan lintas window.
- Aksi baru/ubah:
  - `setHostedSpaces(windowLabel, ids)` / `hydrateForWindow(...)`
  - `assignSpaceToWindow(spaceId, windowLabel)`
  - `unassign/reclaimSpace(spaceId, fallbackLabel)`
- `reorder/rename/setEnv/setColor` tetap operasi metadata, tetapi harus di-broadcast ke window lain.

### 1.3 Binding tab <-> space <-> window (`src/modules/tabs/lib/useTabs.ts`)

- Setiap `Tab` sudah punya `spaceId` — ini tetap binding utama.
- Invariant: **window hanya render/menyimpan tab yang `spaceId`-nya termasuk `hostedSpaceIds` window tersebut**. Tab dari space lain tidak boleh ada di tree lokal.
- `activeId` tab dan state navigasi tetap per-window, tetapi hanya atas subset tab lokal.
- Operasi bulk (`replaceTabs`) harus scoped ke hosted spaces agar tidak menimpa state space lain.

Helper baru:

- `detachTabsForSpace(spaceId)` → `{ detached, remaining, nextActiveId }`
- `attachTabsForSpace(spaceId, tabs, desiredActiveTabId?)`

### 1.4 Binding terminal/PTY <-> window (kritis, backend)

Fakta kode saat ini (`src-tauri/src/modules/pty/mod.rs`, `hub.rs`, `gate.rs`):

- `pty_open` menyimpan `owner = Arc<str>` dari `webview.label()`.
- Session terdaftar ke `Hub` untuk label itu.
- `StreamTokens` diotorisasi per `(label, token)`; request `terax-pty://` ditolak jika mismatch.
- Lifecycle: `reset_webview(label)`, `destroy_webview(label)`, dan cleanup saat window hancur.

Implikasi:

- PTY terikat kuat ke webview owner.
- Hosting space = pemilik PTY untuk semua terminal di space itu.
- Transfer space ke window lain berarti transfer kepemilikan semua PTY space (lihat §4 dan §6).
## 2. Refactor `useTabs` (scope dan ownership)

Target: `src/modules/tabs/lib/useTabs.ts` (+ tipe terkait).

1. **State terpartisi per host**: simpan semua tab tetap di satu sumber (atau terima subset), tetapi API kerja terhadap `hostedSpaceIds`:
   - Inisialisasi/bulk replace hanya dengan tab milik hosted spaces.
   - `openTab/newTab` selalu set `spaceId = activeHostedSpaceId`.
   - Guard: operasi dengan `tabId`/`spaceId` di luar window ini harus no-op + warning (kecuali path transfer khusus).
2. **Ekstrak operasi detach/attach by space**:
   - `detachTabsForSpace(spaceId): { detached: Tab[]; remaining: Tab[]; nextActiveId: number }` mengeluarkan seluruh tab (dan split tree) milik space dari store lokal tanpa dispose PTY di sini (dispose diputuskan alur transfer/close).
   - `attachTabsForSpace(spaceId, tabs, desiredActiveTabId?)` memasukkan set tab ke window target (dipakai saat boot window space atau menerima transfer).
3. **Next active saat detach**: jika active tab ada di space yang dilepas, pilih tab aktif baru dari space hosted yang tersisa di window ini (mirip logika close-tab dalam space).
4. **Bentuk serialized pane tree** (`src/modules/spaces/lib/serialize.ts`) tidak berubah kecuali butuh metadata UI; leaf tidak menyimpan `ptyId` (PTY di-spawn ulang saat attach/render), sehingga restore mudah tetapi *proses shell live tidak bisa dipindah transparan tanpa kerja backend*.
5. **Tab switcher/HUD/command palette** (`App.tsx`): filter daftar tab & aksi ke hosted spaces window ini.

## 3. Frontend UI dan alur (SpaceSwitcher, App, boot/persist)

### 3.1 Aksi UI

`SpaceSwitcher.tsx`:

- Tetap: **"New space"** → `onNewSpace()` (buat di window ini, `hostWindowLabel = current`).
- Tambah item per-space: **"Open in new window"** (ikon `OpenInNewTabIcon`/setara hugeicons) pada row space via dropdown/menu konteks atau tombol sekunder.
- Disabled/berubah jadi **"Focus window"** jika space sudah hosted di window lain.
- Prop baru: `onOpenSpaceInWindow(spaceId)`.

`App.tsx` — `handleOpenSpaceInWindow(spaceId)`:

1. Pastikan state space ter-flush ke disk.
2. Tentukan mode transfer (lihat §6).
3. `invoke("open_space_window", { spaceId })` (command baru).
4. Lepas tab space dari window sumber secara lokal (`detachTabsForSpace`) **hanya setelah** transfer/ownership PTY aman (atau setelah memutuskan respawn); update active space lokal.

### 3.2 Identitas window

- `getCurrentWindow().label` (Tauri) di frontend sebagai `currentWindowLabel`.
- Konvensi label: `"main"` tetap; window space `"space-<spaceId>"` (stabil 1-1, idempotent, mudah untuk capability).

### 3.3 Boot dan persistence

`useSpacesBoot.ts`:

- Terima `windowLabel`. Load semua space dari store, lalu **filter ke space yang `hostWindowLabel === windowLabel`** (plus klaim orphan sesuai §6).
- `replaceTabs` hanya dengan tab milik hosted spaces itu. `setActiveSpaceForNewTabs` ke active space lokal.
- `initialActiveIndex` tetap per-space; active tab window dipilih dari active space lokal.
- `workspaceAuthorize` hanya untuk cwd tab yang benar-benar di-host lokal.

`useSpacePersistence.ts`:

- Saat ini `flush` mengelompokkan semua `tabs` per `spaceId` dan menulis state — berbahaya jika window hanya memegang subset (bisa menimpa state space lain dengan kosong).
- Ubah menjadi **hanya persist space yang di-host window ini** (`hostedSpaceIds`). Jangan pernah menulis state untuk space non-hosted.
- Pertahankan debounce/visibility/beforeunload; tambahkan flush eksplisit sebelum handoff open-in-new-window.
- `saveSpacesList`/`saveActiveId`: operasi metadata boleh dari window mana pun, tetapi harus lewat mekanisme merge/versioning sederhana untuk hindari lost update (§5/§7).
- `LazyStore` (`@tauri-apps/plugin-store`) diakses per proses webview: aman untuk tulis key berbeda, **tidak aman** untuk read-modify-write list yang sama tanpa koordinasi. Gunakan `updatedAt` per space atau tulis granular per key space.
## 4. Perubahan backend (Tauri/Rust)

### 4.1 Capability dan permission (`src-tauri/capabilities/default.json`)

- Saat ini: `"windows": ["main", "settings"]` dengan permission window/event inti.
- Kebutuhan: window space baru harus diizinkan membuat/menggunakan window & event yang sama.
  - Jika versi Tauri proyek mendukung wildcard/glob label, gunakan pola (mis. `"space-*"`).
  - Jika tidak, daftarkan capability untuk label spesifik, atau gunakan satu capability luas untuk app windows — **harus diverifikasi terhadap versi Tauri yang dipakai**.
- Pastikan permission `core:webview:*`/event yang dibutuhkan alur PTY scheme & invoke tersedia untuk label window baru.

### 4.2 Command pembuatan/fokus window (`src-tauri/src/lib.rs`)

Tambah command `open_space_window(app, space_id: String) -> Result<(), String>` (idempotent):

- Jika window dengan label target sudah ada: `show + set_focus` + emit event fokus space, lalu return.
- Buat `WebviewWindowBuilder` seperti `open_settings_window` (reuse logika size/posisi/decorations/transparent + platform-specific titlebar).
- Label konsisten: `format!("space-{space_id}")`.
- URL: route app dengan query, mis. `index.html?windowKind=space&spaceId=...`.

Reuse pola dari `open_settings_window` (`src-tauri/src/lib.rs:89`):

```rust
#[tauri::command]
async fn open_space_window(app: tauri::AppHandle, space_id: String) -> Result<(), String> {
    let label = format!("space-{space_id}");
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("terax:space-focus", space_id);
        return Ok(());
    }
    let url = format!("index.html?windowKind=space&spaceId={}", space_id);
    let builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
        .title("Terax")
        .inner_size(1100.0, 760.0)
        .min_inner_size(720.0, 480.0)
        .resizable(true)
        .visible(false);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.transparent(true).decorations(false);
    let _window = builder.build().map_err(|e| e.to_string())?;
    Ok(())
}
```

Catatan: detail `transparent/decorations/title_bar_style` harus **disamakan dengan window `main`** di `tauri.conf.json` agar UI borderless konsisten; sesuaikan sesuai konfigurasi aktual saat implementasi.

### 4.3 Window lifecycle events (`src-tauri/src/lib.rs`)

- Saat ini `on_window_event` di setup hanya dipasang untuk `"main"` (menghancurkan `"settings"` pada `Destroyed`).
- Tambah penanganan untuk semua window app:
  - `Destroyed` pada window space: **jangan** otomatis destroy window lain. Lakukan cleanup terarah (§4.4 dan §6).
  - Bedakan close `"main"`: memutuskan nasib space window (tutup semua atau tolak close main saat space window terbuka — keputusan produk; rekomendasi: konfirmasi lalu tutup semua child dengan cleanup PTY).
- Emit event internal `terax:window-destroyed` (payload: label) agar frontend dapat reconcile.

### 4.4 PTY/Hub/Workspace backend

- **Tidak perlu ubah protokol stream** untuk kasus dasar jika invariant "PTY milik window host" dijaga: window baru `pty_open` PTY-nya sendiri (owner = label baru) dan mendapat token sendiri.
- Jika dipilih **mode transfer live PTY** (kompleks):
  - Tambah API rebind, mis. `pty_rebind_owner(ids, new_label)`:
    - Update `session.owner`.
    - Pindahkan registrasi `Hub`/`StreamTokens` dari label lama ke baru (revoke/publish).
    - Pastikan tidak ada output drop saat transisi; otorisasi ketat.
  - `reset_webview/destroy_webview` harus idempotent dan aman saat transfer.
- **Mode rekomendasi awal (aman): respawn, bukan transfer live**. Saat space dipindah ke window baru: PTY proses di window lama di-close, tab di window baru membuat session baru (cwd/shell dari state leaf). Kehilangan proses berjalan di terminal tersebut, tetapi tanpa perubahan besar pada PTY core. Harus dikomunikasikan jelas di UX.
- `WorkspaceRegistry` (global app state Rust) sudah shared — aman; pastikan `workspaceAuthorize` tetap dipanggil untuk cwd di window mana pun.
## 5. Sinkronisasi antar-window

Karena ada beberapa webview, metadata & operasi space harus konsisten:

- Mekanisme event Tauri (`emit`/`listen`/`emit_to`):
  - `spaces:meta-changed` (payload: spaces list atau delta + version/updatedAt)
  - `spaces:deleted` (spaceId)
  - `spaces:reordered` (orderedIds)
  - `spaces:host-changed` (spaceId, hostWindowLabel)
  - `spaces:request-focus` (spaceId) saat "Open in new window" untuk window yang sudah terbuka
- Sumber kebenaran: `terax-spaces.json` + event untuk low-latency. Tiap window saat event melakukan reload/merge state relevan lalu re-render.
- **Rename/setColor/setEnv/reorder**: tulis ke store dengan `updatedAt` per space, emit event; window lain merge by id (last-write-wins per field atau per-space `updatedAt`).
- **Delete space**:
  - Hanya dieksekusi window yang meng-host space (atau lewat command backend terkoordinasi) agar cleanup tab/PTY lokal benar.
  - Alur: tutup/lepaskan semua tab & `pty_close`/dispose di host → hapus state + meta → emit `spaces:deleted` → window lain hapus dari UI/list; window space yang menampilkan space itu ditutup atau dialihkan ke fallback.
- **Reorder spaces**: global terhadap daftar; semua window update urutan tampilan.
  - Rekomendasi UI: tampilkan semua space di switcher dengan indikator host window; tombol "Open in new window"/"Focus" untuk non-lokal.
- **Move tab antar-space (`onMoveTabToSpace`)**:
  - Kedua space hosted lokal: perilaku seperti sekarang.
  - Target di-host window lain: **fase awal ditolak/disable** dengan pesan jelas, atau diimplementasi sebagai "close & recreate" di target via command+event (lossy). Perlu keputusan eksplisit.

## 6. Perilaku saat window ditutup

- **Space window ditutup** (opsi A, direkomendasikan):
  - *Reclaim* space kembali ke `"main"`.
  - Sebelum destroy: persist snapshot tab space, stop PTY milik window itu (`destroy_webview(label)`/close sessions), set `hostWindowLabel = "main"`, emit `spaces:host-changed`.
  - Window main (jika ada) reload/attach space atau menandai perlu refresh.
  - Konsekuensi: proses terminal di space itu berhenti (konsisten dengan kepemilikan PTY-per-window).
- Opsi B: tutup space beserta tab/PTY tanpa reclaim ("tutup workspace"), hanya jika UX memang menginginkan.
- Opsi C: larang tutup jika ada foreground process (`pty_has_foreground_process/job`) — konfirmasi di frontend close handler.
- **Main window ditutup**: putuskan (1) menutup seluruh app & semua space window setelah cleanup, atau (2) mencegah/promosikan space window lain. Harus eksplisit. Minimal pastikan `destroy_webview` dipanggil untuk setiap label agar tidak orphan PTY.
- **Selalu**: pada `Destroyed` label, backend menjalankan cleanup setara `destroy_webview(label)` untuk melepas session/token/hub state. Frontend flush persistence saat `beforeunload/visibility` berguna tetapi tidak cukup sendiri.

## 7. Risiko utama dan mitigasi

| Risiko | Mitigasi |
|---|---|
| PTY orphan/leak proses | Hook lifecycle terpusat per label (`Destroyed` → cleanup), idempotent close/reap, audit semua path detach/open-in-new-window agar selalu memutuskan: close PTY sekarang atau transfer ownership tervalidasi. |
| Capability/permission window baru | Verifikasi dukungan glob label Tauri; jika tidak, strategi registrasi capability/label yang pasti. Smoke test: buat window, buka terminal, resize, input/output, close. |
| Race & konsistensi store | Tulis key granular bila memungkinkan, `updatedAt` per record + merge deterministik, serialisasi operasi kritikal (delete/host-change) lewat command backend atau mutex logis (event ack), debounce + flush-before-handoff, reload-on-focus/event. |
| Stream token/origin/hub | Token per label (`StreamTokens`): window baru dapat token sendiri via alur open/poll normal; jangan reuse. Origin check (`AllowedOrigins`) tetap aman untuk app origin. |
| Transfer live PTY | Race antara output in-flight, ack, dan rebind owner/hub; bisa deadlock/drop data. Hindari di fase 1 (respawn), atau desain protokol transfer dengan quiesce + re-register atomik. |
| Persist overwrite | Window yang hanya host subset menulis state kosong untuk space lain. Scope persistence ketat ke `hostedSpaceIds`. |
| UX kehilangan proses | Respawn saat pindah window mematikan job interaktif. Peringatan konfirmasi sebelum pindah jika ada foreground process; dokumentasikan perilaku. |
## 8. Urutan implementasi bertahap dan verifikasi

1. **Fondasi model/store**: tambah `hostWindowLabel` + migrasi + API per-window active/hosted; refactor persistence agar scoped (tanpa UI baru dulu). Tes unit store/serialize.
2. **Boot multi-window**: parameterisasi `useSpacesBoot`/`App` dengan `windowLabel` & filter hosted; pastikan `main` berperilaku identik.
3. **Backend window**: command `open_space_window`, route/label, capability update, lifecycle cleanup `destroy_webview` per label.
4. **UI "Open in new window"**: action di `SpaceSwitcher` + handoff (flush → assign host → detach lokal → buka/fokus window) dengan mode respawn PTY yang jelas.
5. **Sinkronisasi event**: meta-changed/delete/reorder/host-changed + merge; update tampilan switcher lintas window.
6. **Close semantics & edge cases**: reclaim/konfirmasi foreground, perilaku close main, move-tab lintas window (disable/definisi).
7. **(Opsional fase lanjut)**: transfer live PTY/hub rebind jika benar-benar dibutuhkan.

Verifikasi:

- Buka/tutup window space berulang.
- Terminal I/O & resize di window baru.
- Rename/delete/reorder tersinkron.
- Close window → tidak ada PTY/sesi tersisa (`pty_diagnostics`/log).
- Restart app memulihkan hosting yang benar.
- Capability tidak memblokir window berlabel baru.

## 9. Tracking progress

Update checklist ini saat implementasi berjalan. Satu item selesai = centang + catat commit/tanggal.

### Fase 1 — Model data & store
- [x] `SpaceMeta.hostWindowLabel` + migrasi data lama
- [x] Persist active space per window (`activeIdByWindow`)
- [x] `useSpaces`: `selectHostedSpaces`, `assignSpaceToWindow`, hydrate registry penuh
- [x] Tes unit store (migrasi + active per window + hosting)

### Fase 2 — Tab scoping
- [x] `useTabs`: guard scoped ke hosted spaces (`setTabHostingGuard`)
- [x] `detachTabsForSpace` / `attachTabsForSpace` (pure helper + hook action)
- [ ] Filter tab switcher/HUD/command palette — belum eksplisit; switcher/HUD sudah membaca `tabs` yang memang hanya berisi tab lokal
- [x] Tes unit detach/attach

### Fase 3 — Boot & persistence
- [x] `useSpacesBoot` menerima `windowLabel` + filter hosted
- [x] `useSpacePersistence` hanya menulis space yang punya tab di window ini
- [x] Flush eksplisit sebelum handoff (`flushNow()` awaitable)
- [x] Tes regresi restore single-window (focused suite hijau)
- [x] Reclaim orphan space saat cold start `main` (`reclaimOrphanSpaces`, cek `WebviewWindow.getByLabel`): space yang masih ter-host di `space-*` yang sudah tidak ada direbut ke `"main"` sehingga survive restart; space di window yang masih hidup tidak disentuh (aman saat reload `main`)

### Fase 4 — Backend window
- [x] Command `open_space_window` (idempotent: focus jika sudah ada)
- [x] Query `?windowKind=space&spaceId=...` di URL window baru (label `space-<id>`)
- [x] Capability untuk label `space-*` (default/desktop/clipboard)
- [x] Lifecycle cleanup `destroy_webview` per label + reclaim host ke `main`
- [ ] Smoke test window baru (terminal I/O) — butuh run app manual, belum dijalankan di sesi ini

### Fase 5 — UI trigger
- [x] Tombol "Open in new window" di `SpaceSwitcher`
- [x] `handleOpenSpaceInWindow` di `App.tsx`
- [x] Indikator "opened in another window" / "Focus window"
- [x] Konfirmasi jika ada foreground process (best-effort via `leafHasForegroundProcess`)

### Fase 6 — Sinkronisasi
- [x] Event `terax:space-meta-changed` / `terax:space-deleted` / `spaces:host-changed`
- [x] Merge by id + `updatedAt` (`mergeSpacesByUpdatedAt`, `applyHostChanged`)
- [x] Reconcile saat window focus/reload (listener meta-changed + merge registry)

### Fase 7 — Close semantics
- [x] Reclaim space ke `"main"` saat space window ditutup
- [ ] Keputusan close `"main"` + cleanup semua child window — belum ada dialog konfirmasi; child window dibiarkan dan reclaim berjalan saat ditutup
- [x] Move tab lintas window: diblokir dengan toast (disabled by design di v1)

### Fase 8 — Opsional
- [ ] Transfer live PTY / hub rebind (v1 memakai respawn mode)
## 10. Context & keputusan (untuk sesi berikutnya)

- User trigger: **tombol "Open in new window" di space** (bukan "New space").
- Goal: split screen antar space berbeda, state tetap saling sync.
- Keputusan: 1 space = 1 host window; state persisten tetap `terax-spaces.json`; sinkronisasi via Tauri events; PTY fase awal **respawn**, bukan transfer live.
- Catatan penting: `src/main.tsx:30` memanggil `pty_close_all` saat startup. Perilaku ini harus tetap scoped ke webview/window yang bersangkutan (bukan global lintas window), kalau tidak window baru akan mematikan PTY window lain.
- `src-tauri/src/modules/pty/mod.rs:200` mengikat `owner` ke `webview.label()` — jangan diubah sampai fase transfer live PTY.
- Audit 2026-11-15: `control.rs` semula mengirim request ke `"main"` dan readiness-nya global. Sudah diperbaiki menjadi readiness per-window, routing ke owner pane bila ada, fallback ke window terakhir yang fokus, dan error `target_required` untuk CLI eksternal yang ambigu. `agent_events.rs` masih memakai `"main"` hanya di test FakeHost; jalur produksi `emit_to(owner, ...)` sudah window-aware.
- `src-tauri/src/lib.rs` juga memakai `"main"` untuk settings window dan reclaim; ini memang disengaja untuk v1.
- Capability `default.json`, `desktop.json`, `clipboard.json` sekarang mencakup glob `space-*`.
- `src/app/App.tsx` + `src/modules/spaces/*` sudah memakai `windowLabel`, boot scoped, persistence scoped, dan tab hosting guard.
- Batasan v1: handoff memakai respawn mode (PTY lama didispose, tab state dipersist, target me-rehydrate cold tabs), bukan transfer live PTY. Move tab lintas window diblokir; konfirmasi foreground process bersifat best-effort.
- Upstream sync: gunakan `git fetch upstream && git merge upstream/main` pada branch `dydev` secara berkala (merge, bukan rebase-force) agar perubahan tidak hilang.

### Log perubahan dokumen
- 2026-11-15: dibuat dari hasil analisis arsitektur multi-window Terax; checklist progress disertakan.
- 2026-11-15: rencana eksekusi TDD disimpan di `docs/superpowers/plans/2026-11-15-multi-window-space.md`; dokumen ini tetap menjadi ringkasan konteks dan tracking checklist.
