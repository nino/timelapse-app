# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository. `src-tauri/CLAUDE.md` covers the Rust side and `src/CLAUDE.md` the frontend; each module's doc comment says how it works.

## What this app is

A Tauri 2 desktop app (macOS-focused) that runs a background "photographer" loop capturing one screenshot per second of the focused screen, stores them as PNGs under `~/Timelapse/YYYY-MM-DD/00001.png`, and serves a React frontend for scrubbing through any day, whether that day survives as screenshots or only as video. In the background, OCR reads every frame into a searchable index, and a converter encodes each finished hour into a `.mov` and then deletes its PNGs. The photographer starts automatically on app launch (see `src-tauri/src/lib.rs::run`).

## Commands

**The package manager is [Bun](https://bun.sh)** — `bun install`, `bun.lock`. Don't use yarn or npm; there is no `yarn.lock` and `package-lock.json` should never be committed.

Frontend / build:
- `bun run dev` — Vite dev server on port 1420 (Tauri's `beforeDevCommand`)
- `bun run build` — `tsc && vite build` (produces `dist/` consumed by Tauri)
- `bun run tauri dev` / `bun run tauri build` — run/bundle the desktop app
- `bun run lint` / `bun run lint:fix` — oxlint over `.ts`/`.tsx` (Rust dir is ignored)
- `bun run version:bump major|minor` — raises the version floor in **both** `package.json` and `src-tauri/tauri.conf.json` in lockstep (see Releases); always use this rather than editing versions by hand, and never bump the patch in a PR.

Tests:
- `bun test` (also `bun run test`) / `bun run test:watch` / `bun run test:coverage` — Bun's own test runner with happy-dom; Tauri APIs are mocked. Filter with `bun test <path>` or `bun test -t <name>`. There is no Vitest; Vite and oxlint run under Node.
- `cd src-tauri && cargo test` — Rust unit + Tauri command tests. Runs in cloud sessions: the SessionStart hook installs Tauri's Linux libraries and ffmpeg. On a Mac, run `bun run fetch:ffmpeg` once first, because the build needs the bundled ffmpeg. Single test: `cargo test <test_name>`; the frame-source crate alone: `cargo test -p frame_source`.

CI: `.github/workflows/ci.yml` runs on every PR and push to `main`. A Linux job runs lint, `bun run build` and `bun test`; a macOS job builds the frontend (`generate_context!` needs `dist/`), fetches the bundled ffmpeg, puts it on `PATH` for the tests, and runs `cargo test --workspace`.

## Architecture

**Two-process split.** All filesystem and capture work lives in Rust (`src-tauri/src/`, plus the `src-tauri/frame-source/` crate). The React frontend gets everything from Rust (commands, and the `frames:` URI scheme that the `<img>` loads) and uses `@tauri-apps/plugin-fs` only to watch the library for changes.

**Dev and release use different libraries.** Debug builds (`bun run tauri dev`) read and write `~/Timelapse_dev`; release builds use `~/Timelapse`. **Rust owns this decision** (`TIMELAPSE_DIR_NAME` in `src-tauri/src/paths.rs`, switched on `cfg!(debug_assertions)`) and the frontend asks for it via `get_timelapse_root_name`. Never hardcode either name, and never re-derive the root from `import.meta.env.DEV`: it and `cfg!(debug_assertions)` disagree under `tauri dev --release` and `tauri build --debug`, which would point the capture loop at one library and the viewer at the other.

**ffmpeg is bundled; nothing comes from Homebrew** (Nino wants the app self-contained). On macOS, `tauri.macos.conf.json` lists `binaries/ffmpeg` as an `externalBin` sidecar. `scripts/fetch-ffmpeg.ts` (`bun run fetch:ffmpeg`) downloads a pinned, checksummed static build into `src-tauri/binaries/` (gitignored); it runs in `beforeDevCommand`/`beforeBuildCommand`, and `build.rs` stops a macOS build that lacks it. Every ffmpeg call goes through `paths::ffmpeg()`, which prefers the sidecar and falls back to `ffmpeg` on `PATH` (tests, Linux). Never use `ffprobe`; the app doesn't bundle it. To upgrade ffmpeg, change the pin's URL and sha256 together.

**Windows.** Besides the main viewer there are Activity (Window → Activity), Settings (⌘,) and About windows, all on the same `index.html`: `main.tsx` picks the view by window label, and each has its own capability file in `src-tauri/capabilities/`.

## Releases

`.github/workflows/release.yml` builds, signs, notarises and staples the app and a DMG on every push to `main` and publishes them as their own GitHub release, tagged `v<version>` and marked latest; pushing the `beta` tag publishes a prerelease instead. It follows nino/worktree-manager's release workflow and uses the same secret names. The build is Apple Silicon only. Before building, the job runs `bun test` and `cargo test`, after `bun run fetch:ffmpeg`, because `cargo test` doesn't run Tauri's `beforeBuildCommand`.

**PRs never change the version.** Each release to main gets the highest `v*` tag reachable from it plus one patch (`nextVersion` in `scripts/version.ts`), so PRs merged in parallel can't conflict over a version or claim the same one. Only `bun run version:bump major|minor` raises the floor.

**The change log is built from PR titles.** `scripts/changelog.ts` makes one entry per PR on main's first-parent line, shown in the About window and used as the release notes. PRs that only touch docs, CI or tests, dependency bumps and direct pushes are left out. So PR titles are what users read: write them in plain language.

**Auto-update** (`src-tauri/src/updater.rs`, release builds only) installs a newer release only while "Update automatically" is on and only when it needs no admin password; the background loop must never pop up a password prompt unasked. The app trusts only `plugins.updater.pubkey` in `tauri.conf.json`, which must match the `TAURI_SIGNING_PRIVATE_KEY` secret: losing that key means installed copies can never update again.

## Things to know before changing behaviour

- **Screenshots are deleted only after video conversion and OCR.** Nino wants PNGs gone once their image information is kept as video and their text as OCR, never on video alone. `OcrCheck` (`ocr::ocr_check`, over `ocr_progress`) is where OCR says an hour is done; the converter checks it before encoding an hour and again before deleting its PNGs. The video's file name is the only "converted" marker, so deleting a `.mov` before its PNGs go makes that hour convert again.
- **Frame numbers are per day.** `NNNNN` restarts at 1 in every day folder, so anything that stores or looks up a frame keys it on `(day, frame_number)`.
- **The data is live; there is no refresh button**, and there are no separate Images/Videos views: one timeline per day, wherever its frames come from.
- **Performance comes first**, in the UI (resize and scrub smoothness) and in the background work (battery). Prefer dropping an effect or doing less work to keeping something that costs either.

## Conventions (enforced by oxlint — see `.oxlintrc.json`)

- `@typescript-eslint/explicit-function-return-type: error` — every function (including arrow callbacks) needs an explicit return type
- `@typescript-eslint/no-explicit-any: error`
- `import/no-default-export: error` — use named exports. `*.config.ts` / `*.config.js` and `scripts/**` are exempted via an `overrides` block (config files must default-export).
- `no-console: warn` with `warn`/`error` allowed (so `console.log` is a lint warning, not silently fine). Off under `scripts/**`, which is CLI output.
- oxlint's `correctness` category is on as `error`; `react/set-state-in-effect` is demoted to `warn` because some hooks trip it.
- `src-tauri/**` is in `ignorePatterns` — don't try to lint Rust through bun.
- Imports are alphabetised and grouped by hand; oxlint has no `import/order` rule to check it.
