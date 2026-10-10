# The Rust side

Each module's `//!` doc comment says how it works; this file is the map and the rules that cut across modules.

## Modules

- `lib.rs` — the Tauri commands, the `frames://localhost/<YYYY-MM-DD>/<index>` URI scheme (`http://frames.localhost/…` on Windows), the menu, and `setup()`, which creates the library directory, evicts cache folders over 15 days old, and starts the photographer (after a 1 s delay), OCR and the converter. State-backed commands are thin shims over `*_impl(&PhotographerState, …)` functions, which is what the tests call.
- `timelapse.rs` — the `Photographer` capture loop.
- `database.rs` — `ScreenshotDatabase` over `<library>/screenshots.db`, and its migrations.
- `ocr.rs`, `ocr_helper.rs`, `ocr_tiles.rs`, `menu_app.rs` — background OCR with Apple Vision (macOS only), run in a helper process because Vision can break for the rest of a process's life.
- `converter.rs` — encodes each finished hour of PNGs into `<root>/YYYY-MM-DD--HH-MM-SS--hourly.mov` with ffmpeg, then deletes the PNGs.
- `frame-source/` — **a separate crate** (no Tauri dependency) that answers "frame N of day D" whether the frame is a PNG, an hourly video or one of the old script's whole-day videos, with a chunked LRU cache of decoded video frames and read-ahead.
- `activity.rs`, `boost.rs` — what the Activity window shows, and Boost / Low power mode, which change how hard conversion and OCR may work.
- `diagnostics.rs` — `<library>/diagnostics.db`, a log for Nino to upload when something looks wrong (there are no analytics).
- `app_state.rs`, `settings.rs`, `updater.rs`, `paths.rs` — remembered window and viewer state, preferences, auto-update, library and ffmpeg paths.

## Data model

All in `screenshots.db`, keyed by `(day, frame_number)`:
- `screenshots` — one row per stored PNG, with its capture time (`created_at` UTC, `local_time`), `day` folder name, and `window_id` (the app and window in front, NULL when unknown).
- `windows` — one row per distinct app path, app name and window title.
- `ocr_frames` — OCR text, one line per box, boxes packed by `pack_boxes`; one row per frame whose text differs from the day's previous row, so a row stands for every frame up to the next one. `menu_app` is the front app read from the menu bar text, a guess for frames without `window_id`. Searched through the FTS5 index `ocr_fts`.
- `ocr_progress` — per day, the highest frame OCR has handled, with every frame below it handled too. For a day read from video (`by_position`) the numbers are positions in the day's videos instead.
- `video_frames` — for each hourly video, which screenshot each of its frames came from and its capture time, so frames keep their time once the PNGs are gone.

## Things to know before changing behaviour

- **Filename format is load-bearing.** Screenshots are `NNNNN.png` (5-digit, zero-padded). `next_filename` returns the day folder's highest number plus one, and the frame source parses the same format (`parse_screenshot_name`). If you change one, change both. Numbering has gaps (old black frames, files removed by hand), so nothing may assume it is contiguous; the converter hard-links frames into a gapless sequence for ffmpeg.
- **All-black frames are never written.** A black frame is dropped before it is saved, and its number is reused by the next capture.
- **Write files atomically.** A frame is saved to `.NNNNN.png.tmp` and renamed into place; a video is renamed into the library root only once complete; a decoded chunk is staged in `.<chunk>.partial/`. Temp names are dotfiles with other extensions, so nothing that lists frames or videos picks them up.
- **Videos are a day source, not a separate list.** `library::parse_video_name` (frame-source) knows the two name formats, hourly (`YYYY-MM-DD--HH-MM-SS--hourly[-N].mov`, named after the first frame's time) and the old script's whole-day ones (named after the capture day and when the script ran, so not a start time). Anything new that writes videos must use one of them.
- **Hours are bucketed the same way everywhere.** A screenshot's hour is the local hour of its mtime, clamped to its day folder's date; the converter and the frame source both call `frame_source::filed_at`.
- **Never delete today's newest frame.** In today's folder the converter keeps the hour holding the highest-numbered frame, because deleting it would restart the day's numbering at `00001`.
- **ffmpeg is a separate process.** A running encode is paused with `SIGSTOP` on battery and killed by `kill_running_encode` when the app quits or restarts (`RunEvent::Exit`, and before the updater's `app.restart()`, which skips `Exit`); otherwise it would outlive the app.
- **Diagnostics are for changes and errors, never one row per frame.** Per-frame things go into a `Tally` written as one summary an hour.
- **Settings and remembered state.** `settings.json` is shared by dev and release builds, and a new field needs a `#[serde(default)]` value. `state.json` is per profile, because the viewer position names a day in one library.
- **The frontend's file access is scoped.** `capabilities/default.json`'s `fs:scope` permits both library trees two levels deep, and their `.cache/` three, because the scope is baked in at build time and can't branch on the profile. A new path the frontend needs must be added there, or it fails at runtime, not at build time.

## Tests

- Every test that builds a `Photographer` uses `Photographer::new_in` with a `TempDir`, never `Photographer::new()`. `cargo test --release` compiles with `debug_assertions` off, so `new()` would resolve to the real `~/Timelapse`.
- `Photographer::start()` calls `tokio::spawn`, so a test touching it needs `#[tokio::test]` and must call `stop()` before its first `.await`; an `.await` in between lets it capture the real screen.
- OCR tests use a fake `TextRecognizer`; the real one has an `#[ignore]`d test driven by `OCR_TEST_IMAGE`.
