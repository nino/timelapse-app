# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this app is

A Tauri 2 desktop app (macOS-focused) that runs a background "photographer" loop capturing one screenshot per second of the focused screen, stores them as PNGs under `~/Timelapse/YYYY-MM-DD/00001.png`, and serves a React frontend for scrubbing through past days and pre-rendered `.mov` timelapses. The photographer starts automatically on app launch (see `src-tauri/src/lib.rs::run`).

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
- `converter.rs` — the background video converter, started in `setup()` next to the Photographer. It replaces the old `all-timelapses-to-video` script: every PNG is bucketed by the local hour of its mtime, and once an hour has ended it is encoded (the same ffmpeg/libx265 settings the script used) into `<root>/YYYY-MM-DD--HH-MM-SS--hourly.mov` (the time is the first frame's; the `--hourly` tag is what distinguishes these from the old script's videos, whose time is when the script ran), and an hour whose `.mov` exists counts as done. Its PNGs are then deleted, but only when `DeleteCheck` passes; the default check is `false` because screenshots must be OCR'd before they go, so until OCR wires in its own check nothing is deleted. One batch at a time with a 10-minute cool-down, only on AC power (`pmset -g ps` on macOS; any failure reads as battery), ffmpeg under `taskpolicy -b` on macOS, and a running encode is killed if power is unplugged. Frames are hard-linked into `.cache/.convert-<video>/` as a gapless `%05d` sequence first, because ffmpeg's `%05d` input stops at the first numbering gap.
- `extract_video_frames` shells out to `ffmpeg` (must be on PATH) and writes JPEGs into `~/Timelapse/.cache/<video-basename>/frame%06d.jpg`. Re-invocations are no-ops if the cache folder already has frames.

**Frontend — `src/`:**
- `App.tsx` is currently the entire UI. Two view modes (`images` | `videos`) share scrubber/keyboard state. Frames are loaded via `readFile` → `Blob` → `URL.createObjectURL`, and the cleanup effect on `currentImageSrc` calls `revokeObjectURL` to avoid leaks (this is tested).
- `hooks/useFolders.ts` — three hooks reading `~/Timelapse`: `useFolders` lists date dirs (hides dotfiles like `.cache`), `useFiles(folder)` lists files in a date or cache folder, `useVideos` lists `.mov` files. The 5×500ms retry in `useFiles` applies **only** to `.cache/` folders, which ffmpeg may still be filling; a date folder gets one attempt, so a genuine `readDir` failure surfaces immediately instead of ~2s later.
- The frontend talks to Rust via `invoke<T>(...)` for: `extract_video_frames`, `get_screenshot_metadata`, plus the start/stop/error-log commands (not currently wired into the UI but tested).
- Keyboard: ArrowLeft/Right scrub by 1; Shift = 10; Alt/Option = 100. Today's folder auto-selects when present, else the most-recent date.

**Dev and release use different libraries.** Debug builds (`bun run tauri dev`) read and write `~/Timelapse_dev`; release builds (`bun run tauri build`) use `~/Timelapse`. **Rust owns this decision** — `TIMELAPSE_DIR_NAME` in `src-tauri/src/paths.rs`, switched on `cfg!(debug_assertions)` — and the frontend asks for it via the `get_timelapse_root_name` command, resolved in `main.tsx` before the first render. Build paths with `timelapseRoot()` from `src/timelapseRoot.ts`; never hardcode either name, and never re-derive the root from `import.meta.env.DEV`. That was the original design and it was wrong: `import.meta.env.DEV` and `cfg!(debug_assertions)` are different axes that disagree under `tauri dev --release` and `tauri build --debug`, which silently pointed the capture loop at one library and the viewer at the other. `capabilities/default.json` allow-lists **both** trees, because the `fs:scope` list is baked in at build time and cannot branch on the profile — so the isolation is enforced by the root the app asks for, not by the sandbox.

Two consequences worth remembering: `cargo test --release` compiles with `debug_assertions` off, so `TIMELAPSE_DIR_NAME` is the *production* name during release tests — every test must go through `Photographer::new_in` with a `TempDir` rather than `Photographer::new()`. And `setup()` creates the library directory eagerly, before the 1s delay that builds the Photographer, because the frontend's first `readDir` would otherwise fail on a fresh library.

**Tauri ↔ frontend trust boundary.** The `fs` plugin's scope (`capabilities/default.json`) only permits `$HOME/Timelapse` up to two levels deep. If you add a new path the frontend needs to read, extend the `fs:scope` allow-list — otherwise `readFile`/`readDir` will fail at runtime, not at build time.


**Test isolation seams.** `Photographer::new()` resolves the profile's library root and delegates to `Photographer::new_in(root)`; tests always use `new_in` with a `TempDir` so `cargo test` never reads, writes, or captures into a real library, in either profile. `Photographer::start()` calls `tokio::spawn`, so anything touching it needs `#[tokio::test]`, and the test must call `stop()` before its first yield point — that is what makes the capture loop exit on its first poll. An `.await` between start and stop lets it capture the real screen.
## Conventions (enforced by oxlint — see `.oxlintrc.json`)

- `@typescript-eslint/explicit-function-return-type: error` — every function (including arrow callbacks) needs an explicit return type
- `@typescript-eslint/no-explicit-any: error`
- `import/no-default-export: error` — use named exports. `*.config.ts` / `*.config.js` and `scripts/**` are exempted via an `overrides` block (config files must default-export), so they no longer need per-line disable comments.
- `no-console: warn` with `warn`/`error` allowed (so `console.log` is a lint warning, not silently fine). Off under `scripts/**`, which is CLI output.
- oxlint's `correctness` category is on as `error`; `react/set-state-in-effect` is demoted to `warn` because the existing hooks trip it (7 sites in `App.tsx`/`useFolders.ts`).
- `src-tauri/**` is in `ignorePatterns` — don't try to lint Rust through bun.
- **Not enforced any more:** `import/order` (alphabetised, grouped imports) — oxlint has no equivalent rule. The convention still holds by hand; it is just no longer machine-checked.

## Things to know before changing behaviour

- **Filename format is load-bearing.** Screenshots are `NNNNN.png` (5-digit, zero-padded). `next_filename` scans the dir, parses every numeric stem, and returns `max + 1`. The frontend parses the same format to look up DB metadata (`parseInt(filename.replace(".png", ""), 10)`). If you change one, change both.
- **All-black detection deletes files.** `is_image_all_black` runs after every capture; if true, the PNG is removed and the loop sleeps 10s. Expect gaps in the numbering — `next_filename` handles them.
- **Frame extraction is cached forever (until eviction).** `extract_video_frames` only re-runs ffmpeg when the cache folder is missing or empty. To force re-extraction, delete `~/Timelapse/.cache/<video-basename>/`.
- **Extraction stages, then publishes.** ffmpeg writes into `.cache/.<video-basename>.partial/` and that folder is renamed into place only once it exits successfully with at least one frame (`publish_staged_frames`). So the presence of `.cache/<video-basename>/` means a *complete* run — an interrupted one leaves a `.partial` folder that is cleared on the next attempt and evicted like any other cache dir. Don't reintroduce writing frames straight to the published path: that made every killed extraction into a permanent partial cache hit.
- **Videos are listed newest-first in the dropdown.** `useVideos` returns chronological order (oldest first) and `App.tsx` reverses it at the render site, matching the images date picker; the auto-select takes `videos[videos.length - 1]` off the hook's order. Three places have to agree — the hook's sort, the auto-select index, and the dropdown.
- **Screenshots are deleted only after video conversion and OCR.** Nino wants PNGs gone once their image information is kept as video and their text as OCR — never on video alone. `DeleteCheck` is where OCR says an hour is done. The video's file name is the only "converted" marker, so deleting a `.mov` before its PNGs go makes that hour convert again. In today's folder the hour holding the highest-numbered frame is never deleted, because `next_filename` is `max + 1` and deleting the newest frame would restart today's numbering at `00001`.
- **The DB lives next to the screenshots.** `~/Timelapse/screenshots.db` (or `~/Timelapse_dev/screenshots.db` in a debug build). Don't move it without updating `Photographer::new_in` and the migration logic.
