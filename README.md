# Timelapse App

A macOS app that takes a screenshot of your screen once a second, keeps it on
your Mac, and lets you scrub back through any day and search for any text that
was on screen.

Everything happens on your machine: capture, text recognition (Apple's Vision
framework) and video encoding (a bundled ffmpeg). Nothing is uploaded, and
there are no analytics.

## Features

- **Continuous capture.** One frame a second of the screen holding the focused
  window, starting when the app launches. All-black frames (a locked or
  sleeping screen) are skipped. Each frame also records which app and window
  were in front.
- **One timeline per day.** Pick a day and scrub through it with the slider or
  the keyboard, or play it back at a chosen speed. Recent hours are still
  screenshots and older ones are video; the viewer shows both on the same
  timeline. New captures appear as they happen, with no refresh.
- **Search.** Every frame's text is read on-device into a full-text index.
  The find bar (⌘F) marks matches on the scrubber, steps through them with
  Enter, and outlines the matching lines in the frame on screen.
- **Video conversion.** Once an hour is over and its text has been read, the
  hour's screenshots are encoded into a `.mov` and the PNGs are deleted. A
  screenshot is never deleted before both its image (as video) and its text
  (as OCR) are kept.
- **Battery-friendly background work.** OCR and video conversion run at
  background priority and only on AC power; an encode in progress pauses when
  you unplug. The Activity window (Window → Activity) shows what they are
  doing and offers **Boost** (work at full speed for a while, optionally on
  battery) and **Low power** (pause both for a while, even on AC power).
- **Automatic updates.** Release builds check GitHub for a newer release at
  launch and hourly, and install it while "Update automatically" is on in
  Settings (⌘,). They never ask for an admin password in the background.
  "Check for Updates…" in the app menu works either way.

### Keyboard

| Keys | Action |
| --- | --- |
| ← / → | Previous / next frame |
| Shift + ← / → | 10 frames |
| Option + ← / → | 100 frames |
| ⌘ + ← / → | Previous / next day |
| Space | Play / pause |
| ⌘F | Find |
| Enter / Shift + Enter | Next / previous match |

## Install

Download the DMG from the [latest release](https://github.com/nino/timelapse-app/releases/latest)
and drag the app to `/Applications` or `~/Applications`. Builds are signed and
notarised, for Apple Silicon only.

On first launch, give the app **Screen Recording** permission in System
Settings → Privacy & Security. Without it macOS only lets it capture the
desktop wallpaper.

If you install into a folder your account can't write to (for example
`/Applications` as a non-admin user), automatic updates are skipped; use
"Check for Updates…" instead, which asks for the password. `~/Applications`
avoids this.

## Where your data lives

Everything is in `~/Timelapse`:

| Path | What it is |
| --- | --- |
| `YYYY-MM-DD/00001.png` | Screenshots, numbered per day, until their hour is converted |
| `YYYY-MM-DD--HH-MM-SS--hourly.mov` | One video per converted hour, named after its first frame |
| `screenshots.db` | SQLite: capture times, front app and window, OCR text and its search index |
| `diagnostics.db` | A log of state changes, errors and hourly summaries, kept for 30 days, for troubleshooting |
| `.cache/` | Decoded video frames for the viewer; folders unused for 15 days are removed |

Older whole-day videos from the earlier `timelapse-to-video` scripts in the same
folder are picked up too, and their text is read from the video.

Preferences (`settings.json`) and remembered window positions (`state.json`)
are in the app's config folder,
`~/Library/Application Support/com.timelapse-app.app/`.

Development builds use `~/Timelapse_dev` instead, so working on the app never
touches your real library.

## Building from source

You need [Bun](https://bun.sh) and a Rust toolchain. Nothing else is installed
system-wide: ffmpeg is downloaded (a pinned, checksummed static build) into
`src-tauri/binaries/` and bundled into the app.

```bash
bun install
bun run tauri dev      # run the app (fetches ffmpeg first)
bun run tauri build    # build a release bundle
```

Other commands:

```bash
bun run lint           # oxlint
bun test               # frontend tests (Bun's runner, happy-dom)
bun run fetch:ffmpeg   # fetch the bundled ffmpeg on its own
cd src-tauri && cargo test --workspace   # Rust tests
```

On a Mac, run `bun run fetch:ffmpeg` once before `cargo test`, because the
build needs the bundled ffmpeg. On Linux the Rust tests build against Tauri's
Linux libraries and use ffmpeg from `PATH`; text recognition only runs on
macOS, and its tests use a fake recognizer. All tests run against temporary
folders and never read your library or capture the screen. See
[TESTING.md](TESTING.md) for what they cover.

### Layout

- `src/` — the React viewer, and the Activity, Settings and About windows.
- `src-tauri/src/` — capture, OCR, video conversion, the database, updates.
- `src-tauri/frame-source/` — a separate crate that returns frame N of a day
  from a PNG or a video, with a cache of decoded video frames.
- `scripts/` — ffmpeg download, versioning and change log.

[`CLAUDE.md`](CLAUDE.md), [`src/CLAUDE.md`](src/CLAUDE.md) and
[`src-tauri/CLAUDE.md`](src-tauri/CLAUDE.md) describe the architecture and the
rules to keep when changing it.

## Releases

Every push to `main` builds, signs and notarises the app and publishes it as a
GitHub release tagged `v<version>`, which installed copies then update to.
Pushing the `beta` tag publishes a prerelease instead.

- **Pull requests never change the version.** Each release gets the highest
  existing `v*` tag plus one patch, so PRs merged in parallel can't clash.
  `bun run version:bump major|minor` raises the version floor when needed.
- **The change log is made from PR titles.** Each PR merged to `main` becomes
  one entry in the About window and the release notes, so a PR title should
  say in plain language what changes for someone using the app. PRs that only
  touch docs, CI or tests, and dependency bumps, are left out.
