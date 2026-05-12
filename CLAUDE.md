# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this app is

A Tauri 2 desktop app (macOS-focused) that runs a background "photographer" loop capturing one screenshot per second of the focused screen, stores them as PNGs under `~/Timelapse/YYYY-MM-DD/00001.png`, and serves a React frontend for scrubbing through past days and pre-rendered `.mov` timelapses. The photographer starts automatically on app launch (see `src-tauri/src/lib.rs::run`).

## Commands

Frontend / build:
- `yarn dev` — Vite dev server on port 1420 (Tauri's `beforeDevCommand`)
- `yarn build` — `tsc && vite build` (produces `dist/` consumed by Tauri)
- `yarn tauri dev` / `yarn tauri build` — run/bundle the desktop app
- `yarn lint` / `yarn lint:fix` — ESLint over `.ts`/`.tsx` (Rust dir is ignored)
- `yarn version:bump [major|minor|patch]` — updates **both** `package.json` and `src-tauri/tauri.conf.json` in lockstep; always use this rather than editing versions by hand

Tests:
- `yarn test` (watch) / `yarn test:run` (CI) / `yarn test:ui` / `yarn test:coverage` — Vitest with happy-dom; Tauri APIs are mocked in tests
- `cd src-tauri && cargo test` — Rust unit + Tauri command tests (uses `tempfile` for filesystem isolation)
- Single Rust test: `cargo test <test_name>` from `src-tauri/`

## Architecture

**Two-process split.** All filesystem and capture work lives in Rust (`src-tauri/src/`); the React frontend reads files from `~/Timelapse` directly via `@tauri-apps/plugin-fs` (scoped in `src-tauri/capabilities/default.json`) and invokes Rust commands only for things that require it (capture control, ffmpeg frame extraction, DB lookups).

**Rust side — `src-tauri/src/`:**
- `lib.rs` — defines `#[tauri::command]`s and holds the `PhotographerState = Arc<Mutex<Option<Photographer>>>`. The Photographer is auto-started in `setup()` after a 1s delay; `evict_old_cache()` runs first to drop cache folders older than 15 days.
- `timelapse.rs` — the `Photographer` runs a tokio loop: capture focused screen → resize via MagickWand → drop if all-black → name as `NNNNN.png` (next-after-max in the day dir) → write a row to SQLite. Black images get a 10s backoff; errors get 60s and are appended to a bounded in-memory log (max 10 000 entries).
- `database.rs` — `ScreenshotDatabase` wraps a `rusqlite` connection at `~/Timelapse/screenshots.db`. Has its own `migrations` table; the one existing migration (`split_timestamps`) splits the legacy `creation_date` column into `created_at` (UTC) and `local_time` (Local). New migrations should follow that pattern: check applied → run → record.
- `extract_video_frames` shells out to `ffmpeg` (must be on PATH) and writes JPEGs into `~/Timelapse/.cache/<video-basename>/frame%06d.jpg`. Re-invocations are no-ops if the cache folder already has frames.

**Frontend — `src/`:**
- `App.tsx` is currently the entire UI. Two view modes (`images` | `videos`) share scrubber/keyboard state. Frames are loaded via `readFile` → `Blob` → `URL.createObjectURL`, and the cleanup effect on `currentImageSrc` calls `revokeObjectURL` to avoid leaks (this is tested).
- `hooks/useFolders.ts` — three hooks reading `~/Timelapse`: `useFolders` lists date dirs (hides dotfiles like `.cache`), `useFiles(folder)` lists files in a date or cache folder (with a 5×500ms retry for freshly-created cache folders), `useVideos` lists `.mov` files.
- The frontend talks to Rust via `invoke<T>(...)` for: `extract_video_frames`, `get_screenshot_metadata`, plus the start/stop/error-log commands (not currently wired into the UI but tested).
- Keyboard: ArrowLeft/Right scrub by 1; Shift = 10; Alt/Option = 100. Today's folder auto-selects when present, else the most-recent date.

**Tauri ↔ frontend trust boundary.** The `fs` plugin's scope (`capabilities/default.json`) only permits `$HOME/Timelapse` up to two levels deep. If you add a new path the frontend needs to read, extend the `fs:scope` allow-list — otherwise `readFile`/`readDir` will fail at runtime, not at build time.

## Conventions (enforced by ESLint — see `eslint.config.js`)

- `@typescript-eslint/explicit-function-return-type: error` — every function (including arrow callbacks) needs an explicit return type
- `@typescript-eslint/no-explicit-any: error`
- `import/no-default-export: error` — use named exports. `vite.config.ts`, `vitest.config.ts`, and `eslint.config.js` are the only files with `// eslint-disable-next-line` for the default export they require.
- `import/order` with alphabetised groups and blank lines between groups
- `no-console: warn` with `warn`/`error` allowed (so `console.log` is a lint warning, not silently fine)
- `src-tauri/**` is in the ESLint ignore list — don't try to lint Rust through yarn.

## Things to know before changing behaviour

- **Filename format is load-bearing.** Screenshots are `NNNNN.png` (5-digit, zero-padded). `next_filename` scans the dir, parses every numeric stem, and returns `max + 1`. The frontend parses the same format to look up DB metadata (`parseInt(filename.replace(".png", ""), 10)`). If you change one, change both.
- **All-black detection deletes files.** `is_image_all_black` runs after every capture; if true, the PNG is removed and the loop sleeps 10s. Expect gaps in the numbering — `next_filename` handles them.
- **Frame extraction is cached forever (until eviction).** `extract_video_frames` only re-runs ffmpeg when the cache folder is missing or empty. To force re-extraction, delete `~/Timelapse/.cache/<video-basename>/`.
- **The DB lives next to the screenshots.** `~/Timelapse/screenshots.db`. Don't move it without updating `Photographer::new` and the migration logic.
