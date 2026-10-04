# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this app is

A Tauri 2 desktop app (macOS-focused) that runs a background "photographer" loop capturing one screenshot per second of the focused screen, stores them as PNGs under `~/Timelapse/YYYY-MM-DD/00001.png`, and serves a React frontend for scrubbing through any day, whether that day survives as screenshots or only as a rendered `.mov` timelapse. The photographer starts automatically on app launch (see `src-tauri/src/lib.rs::run`).

## Commands

**The package manager is [Bun](https://bun.sh)** — `bun install`, `bun.lock`. Don't use yarn or npm; there is no `yarn.lock` and `package-lock.json` should never be committed.

Frontend / build:
- `bun run dev` — Vite dev server on port 1420 (Tauri's `beforeDevCommand`)
- `bun run build` — `tsc && vite build` (produces `dist/` consumed by Tauri)
- `bun run tauri dev` / `bun run tauri build` — run/bundle the desktop app
- `bun run lint` / `bun run lint:fix` — oxlint over `.ts`/`.tsx` (Rust dir is ignored)
- `bun run version:bump [major|minor|patch]` — updates **both** `package.json` and `src-tauri/tauri.conf.json` in lockstep; always use this rather than editing versions by hand. Bun runs the TypeScript file directly, so there is no `tsx`/`ts-node` step.

Tests:
- `bun run test` (watch) / `bun run test:run` (CI) / `bun run test:ui` / `bun run test:coverage` — Vitest with happy-dom; Tauri APIs are mocked in tests
- `cd src-tauri && cargo test` — Rust unit + Tauri command tests (uses `tempfile` for filesystem isolation)
- Single Rust test: `cargo test <test_name>` from `src-tauri/`

Note: the scripts run Vitest/Vite/oxlint under **Node**, not the Bun runtime — Bun is the package manager and script runner here. `bun run test:run` is not the same thing as `bun test` (Bun's own test runner), which this project does not use.

## Architecture

**Two-process split.** All filesystem and capture work lives in Rust (`src-tauri/src/`); the React frontend reads files from `~/Timelapse` directly via `@tauri-apps/plugin-fs` (scoped in `src-tauri/capabilities/default.json`) and invokes Rust commands only for things that require it (capture control, ffmpeg frame extraction, DB lookups).

**Rust side — `src-tauri/src/`:**
- `lib.rs` — defines `#[tauri::command]`s and holds the `PhotographerState = Arc<Mutex<Option<Photographer>>>`. Each state-backed command is a thin shim over a `*_impl(&PhotographerState, …)` function; `tauri::State` wraps a private reference with no public constructor, so that split is what makes the logic unit-testable. The Photographer is auto-started in `setup()` after a 1s delay; `evict_old_cache()` runs first to drop cache folders older than 15 days.
- `timelapse.rs` — the `Photographer` runs a tokio loop: capture focused screen → resize via MagickWand → drop if all-black → name as `NNNNN.png` (next-after-max in the day dir) → write a row to SQLite. Black images get a 10s backoff; errors get 60s and are appended to a bounded in-memory log (max 10 000 entries).
- `database.rs` — `ScreenshotDatabase` wraps a `rusqlite` connection at `~/Timelapse/screenshots.db`. Has its own `migrations` table; the one existing migration (`split_timestamps`) splits the legacy `creation_date` column into `created_at` (UTC) and `local_time` (Local). New migrations should follow that pattern: check applied → run → record.
- `frame-source/` — **a separate crate** (workspace member, no Tauri or MagickWand dependency) that answers "frame N of day D" and hides where frames come from. A day is served from its screenshots if it has any, otherwise from its videos (`YYYY-MM-DD--HH-MM-SS.mov|mp4`, in the library root or the day folder) played back to back in start order; byte-identical duplicate videos and files ffprobe can't read are skipped. Video frames are decoded by ffmpeg in chunks of `CHUNK_FRAMES` (150) into a size-capped LRU cache under the OS cache dir (`<app cache>/<Timelapse|Timelapse_dev>/frames/`, 2 GiB), staged and renamed into place like the old extraction. Timestamps come from the DB keyed by **(day, frame number)**, then file mtime; video frames get an estimate from the file name's start time (`exact: false`). Test it with `cargo test -p frame_source` — it needs only ffmpeg, so it runs where the app crate can't build.
- `lib.rs` exposes it through the `list_days`, `get_day` and `get_frame_time` commands and a `frames://localhost/<YYYY-MM-DD>/<index>` URI scheme (`http://frames.localhost/…` on Windows) that the `<img>` loads directly.

**Frontend — `src/`:**
- `App.tsx` is the entire UI: one view, a day picker (newest first, today auto-selected) and a scrubber. There are no Images/Videos tabs; whether a day is screenshots or video is the frame source's business. The `<img>` points at `frameUrl(day, index)` from `src/frames.ts`, gated by `useGatedImage` so only one frame loads at a time and each finished load jumps to wherever the scrubber is now (otherwise dragging across a video day queues a decode per chunk).
- `hooks/useLibrary.ts` — `useDays()` (via `list_days`) and `useDay(date)` (via `get_day`: frame count and source). `useDay` returns `null` while a newly selected day loads, never the previous day's answer.
- **The data is live; there is no refresh button.** Both hooks re-ask Rust when `useDirectoryChanges` (`hooks/useDirectoryChanges.ts`) reports a change in the library root or the day folder. It uses the fs plugin's debounced `watch` (Cargo feature `watch`, permissions `fs:allow-watch`/`fs:allow-unwatch`) and falls back to polling every 3s if the watcher can't be created. `useLatestLoad` makes sure only the latest request per hook may publish, and reloads that find nothing new keep the old value so effects don't re-run every second.
- **The viewer follows the live edge.** The position is `{ day, index }` with `index: null` meaning "newest frame", so new captures are followed for free and scrubbing back stops following; a position recorded for another day counts as null, so opening a day lands on its last frame without first requesting frame 0. At midnight the selection moves to the new day only for someone following the previous newest day's live edge.
- The frontend talks to Rust via `invoke<T>(...)` for `list_days`, `get_day` and `get_frame_time` (wrapped in `src/frames.ts`), plus the start/stop/error-log commands (not currently wired into the UI but tested). `get_screenshot_metadata` is no longer used by the UI: it looks frames up by number alone, which collides across days.
- Keyboard: ArrowLeft/Right scrub by 1; Shift = 10; Alt/Option = 100, including when the slider has focus (the handler prevents its native step). Arrows in the day picker are left alone.

**Dev and release use different libraries.** Debug builds (`bun run tauri dev`) read and write `~/Timelapse_dev`; release builds (`bun run tauri build`) use `~/Timelapse`. **Rust owns this decision** — `TIMELAPSE_DIR_NAME` in `src-tauri/src/paths.rs`, switched on `cfg!(debug_assertions)` — and the frontend asks for it via the `get_timelapse_root_name` command, resolved in `main.tsx` before the first render. Build paths with `timelapseRoot()` from `src/timelapseRoot.ts`; never hardcode either name, and never re-derive the root from `import.meta.env.DEV`. That was the original design and it was wrong: `import.meta.env.DEV` and `cfg!(debug_assertions)` are different axes that disagree under `tauri dev --release` and `tauri build --debug`, which silently pointed the capture loop at one library and the viewer at the other. `capabilities/default.json` allow-lists **both** trees, because the `fs:scope` list is baked in at build time and cannot branch on the profile — so the isolation is enforced by the root the app asks for, not by the sandbox.

Two consequences worth remembering: `cargo test --release` compiles with `debug_assertions` off, so `TIMELAPSE_DIR_NAME` is the *production* name during release tests — every test must go through `Photographer::new_in` with a `TempDir` rather than `Photographer::new()`. And `setup()` creates the library directory eagerly, before the 1s delay that builds the Photographer, because the frontend's first `readDir` would otherwise fail on a fresh library.

**Tauri ↔ frontend trust boundary.** The `fs` plugin's scope (`capabilities/default.json`) only permits `$HOME/Timelapse` up to two levels deep. If you add a new path the frontend needs to read, extend the `fs:scope` allow-list — otherwise `readFile`/`readDir` will fail at runtime, not at build time.


**Test isolation seams.** `Photographer::new()` resolves the profile's library root and delegates to `Photographer::new_in(root)`; tests always use `new_in` with a `TempDir` so `cargo test` never reads, writes, or captures into a real library, in either profile. `Photographer::start()` calls `tokio::spawn`, so anything touching it needs `#[tokio::test]`, and the test must call `stop()` before its first yield point — that is what makes the capture loop exit on its first poll. An `.await` between start and stop lets it capture the real screen.
## Conventions (enforced by oxlint — see `.oxlintrc.json`)

- `@typescript-eslint/explicit-function-return-type: error` — every function (including arrow callbacks) needs an explicit return type
- `@typescript-eslint/no-explicit-any: error`
- `import/no-default-export: error` — use named exports. `*.config.ts` / `*.config.js` and `scripts/**` are exempted via an `overrides` block (config files must default-export), so they no longer need per-line disable comments.
- `no-console: warn` with `warn`/`error` allowed (so `console.log` is a lint warning, not silently fine). Off under `scripts/**`, which is CLI output.
- oxlint's `correctness` category is on as `error`; `react/set-state-in-effect` is demoted to `warn` because some hooks trip it.
- `src-tauri/**` is in `ignorePatterns` — don't try to lint Rust through bun.
- **Not enforced any more:** `import/order` (alphabetised, grouped imports) — oxlint has no equivalent rule. The convention still holds by hand; it is just no longer machine-checked.

## Things to know before changing behaviour

- **Filename format is load-bearing.** Screenshots are `NNNNN.png` (5-digit, zero-padded). `next_filename` scans the dir, parses every numeric stem, and returns `max + 1`. The frame source parses the same format (`parse_screenshot_name`) to order frames and look up timestamps. If you change one, change both.
- **All-black detection deletes files.** `is_image_all_black` runs after every capture; if true, the PNG is removed and the loop sleeps 10s. Expect gaps in the numbering — `next_filename` handles them.
- **Legacy videos are a day source, not a separate list.** Most of the real library is `YYYY-MM-DD--HH-MM-SS.mov` files in the root whose PNGs were deleted by an old script; that name format is what `library::parse_video_name` keys on. Anything that writes videos (e.g. automatic conversion) should use the same name, with the time of the first frame, and rename into place only when complete.
- **Decoded chunks stage, then publish.** ffmpeg writes a chunk into `.<chunk>.partial/` and it is renamed into place only on success, so a chunk folder that exists is whole. `ChunkCache::open` clears leftover partials. The legacy `~/Timelapse/.cache/` from the old `extract_video_frames` command is no longer written; `evict_old_cache` still clears it out over 15 days.
- **The DB lives next to the screenshots.** `~/Timelapse/screenshots.db` (or `~/Timelapse_dev/screenshots.db` in a debug build). Don't move it without updating `Photographer::new_in` and the migration logic.
