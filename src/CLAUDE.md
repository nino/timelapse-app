# The frontend

## Structure

- `App.tsx` is the whole viewer: a day picker (newest first), a find bar, and a scrubber with play/pause and an fps picker. The `<img>` loads `frameUrl(day, index)` from `frames.ts` through `useGatedImage`, so only one frame loads at a time and each finished load jumps to wherever the scrubber is now; otherwise dragging across a video day would queue a decode per chunk.
- `main.tsx` resolves the library root name (`get_timelapse_root_name`) and the saved viewer position before the first render, then renders `App`, `ActivityView`, `SettingsView` or `AboutView` by window label.
- Rust commands are wrapped in small modules (`frames.ts`, `search.ts`, `activity.ts`, `settings.ts`, `diagnostics.ts`, `viewerPosition.ts`). Build library paths with `timelapseRoot()` from `timelapseRoot.ts`.
- `hooks/useLibrary.ts` (`useDays`, `useDay`) re-asks Rust whenever `useDirectoryChanges` reports a change in the library root or the day folder (the fs plugin's `watch`, falling back to polling every 3 s). `useDay` returns `null` while a newly selected day loads, never the previous day's answer, and reloads that find nothing new keep the old value so effects don't re-run.
- `hooks/useOcrSearch.ts` drives the find bar; `hooks/usePendingFrames.ts` marks the video stretches not decoded yet, which the scrubber draws paler.
- Don't call `get_screenshot_metadata`: it looks frames up by number alone, which collides across days.

## Behaviour to keep

- **The viewer follows the live edge.** The position is `{ day, index }`, and `index: null` means "newest frame", so new captures are followed for free and scrubbing back stops following. A position recorded for another day counts as null, so opening a day lands on its last frame without first loading frame 0. At midnight the selection moves to the new day only for someone following the previous day's live edge.
- **Keyboard.** ArrowLeft/Right scrub by 1, Shift by 10, Alt/Option by 100 (also when the slider has focus). Cmd/Ctrl+ArrowLeft/Right open the previous/next day. Cmd/Ctrl+F focuses the find bar. Arrows in the day picker and the find bar are left alone. Space plays and pauses, except in the find bar, the day picker or on a focused button.
- **Playback** asks for the next frame only once the current one has loaded or failed, so a slow decode slows playback instead of queueing requests. It stops at the day's last frame, when another day opens, and when the window is hidden; play on the last frame starts the day over. The fps choice (default 15) is remembered in `localStorage`.
- **Find bar.** Matches are marked above the scrubber, back-to-back matches merge into one stop, Enter/Shift+Enter step through them, and the matching lines of the frame on screen are outlined. Searches re-run only when `get_ocr_version` changes, not on every capture.

## Styling

Styling follows Nino's Ninoes design system (`nino/ninoes`), copied in: the tokens (`bg-page`, `bg-card`, `text-fg`, `text-muted-fg`, `border-border`, `bg-primary`, …) are in `@theme` in `App.css` and redefined for dark mode there, so use them instead of raw colours; shared class strings are in `ui.ts`. The colour values are Nino's own higher-contrast "Ultramarine" palette, not Ninoes', with a cyan playhead bar (`--color-playhead`) as the scrubber thumb. Don't use blurred shadows or the squircle `corner-shape`: WebKit repaints a blurred shadow on the CPU on every frame of a window resize, and Chromium paints squircles three times slower. Fonts are bundled with `@fontsource`, never loaded from the network.

## Tests

`bunfig.toml` preloads `test/happydom.ts` (must load first) and then `test/setup.ts` (jest-dom matchers, Tauri mocks, `URL.createObjectURL` stubs, `cleanup`). Import test APIs from `bun:test`.
- **`mock.module` is process-wide.** Bun runs every test file in one process, so a module mocked in one file is mocked for all of them. The Tauri modules are mocked once, in `setup.ts`, with every export any test needs; add new exports there. To fake one of the app's own modules, use `spyOn(namespace, 'fn')` and `mockRestore()` it in `afterAll`, as `App.test.tsx` does.
- **No `vi.mocked`.** Use `mocked(fn)` from `test/mocked.ts`.
- The day picker is a Headless UI `Listbox`: tests open it by clicking the `Day` button and then the option.
