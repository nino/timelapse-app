# Testing Documentation

This document describes the comprehensive test suite for the timelapse-app.

## Overview

The project includes extensive test coverage for both frontend (React/TypeScript) and backend (Rust) code:

- **Frontend Tests**: React components and hooks
- **Rust Tests**: core functionality and Tauri commands

Both suites are hermetic. Every Rust test builds its `Photographer` through
`Photographer::new_in` against a `TempDir`, so no test reads or writes a real
library and none captures the screen. This holds in **both** profiles — worth
stating explicitly, because `cargo test --release` turns `debug_assertions` off,
which flips `TIMELAPSE_DIR_NAME` from `Timelapse_dev` to the production
`Timelapse`; any test that called `Photographer::new()` would open the real
database. Verified by running both profiles with `HOME` pointed at an empty
directory and confirming nothing is created inside it.

## Frontend Tests

### Technology Stack
- **Test Runner**: `bun test` (Bun's built-in runner)
- **Testing Library**: @testing-library/react 16.3
- **Environment**: happy-dom, registered globally by `@happy-dom/global-registrator`
- **Assertions**: @testing-library/jest-dom matchers

### Test Files

#### `src/hooks/useLibrary.test.ts`
`useDays` and `useDay`, with `invoke` and the fs `watch` mocked so a test can
play the filesystem:
- ✓ Lists days and picks up new ones when the library root changes
- ✓ Keeps the same array when nothing changed
- ✓ Reports errors and clears them on a successful reload
- ✓ Loads a day and follows new captures in its folder
- ✓ Never shows the previous day while a new one loads
- ✓ Ignores a slow answer for a day that is no longer selected

#### `src/hooks/useDirectoryChanges.test.ts`
- ✓ Calls back when the watcher reports a change, with a 500 ms debounce
- ✓ Stops watching on unmount, even if the watcher arrives late
- ✓ Falls back to polling every 3 s when the watcher cannot be created
- ✓ `useLatestLoad` only lets the most recent call publish

#### `src/frames.test.ts`
- ✓ `frameUrl` uses `frames://localhost/…` on macOS and `http://frames.localhost/…` on Windows

#### `src/App.test.tsx`
The library hooks and `getFrameTime` are spied on, so App sees whatever
library a test describes:
- ✓ Library and day errors, an empty library, an empty day
- ✓ One view: no tabs, no refresh button; a video day behaves like a screenshot day
- ✓ Opens the newest day on its last frame; days listed newest first, today marked
- ✓ Scrubbing by slider and keys (1 / Shift 10 / Option 100, also on a focused slider)
- ✓ Only one frame loads at a time, then it skips to the latest position
- ✓ Exact and estimated (`~HH:MM`) capture times
- ✓ Follows new captures on the live edge, stays put when scrubbed back, rolls over at midnight only when following

#### `src/timelapseRoot.test.ts`
Guards the library-root contract:
- ✓ Throws rather than guessing when the root is unresolved
- ✓ Takes the root from Rust, not from the bundler environment

Note: `src/test/setup.ts` pins the root to `Timelapse_test_root` — deliberately
neither production name — so any path built from a hardcoded `"Timelapse"`
literal fails the suite instead of passing silently.

### Running Frontend Tests

```bash
# Run tests once
bun test

# Run one file, or tests whose name matches a pattern
bun test src/hooks/useLibrary.test.ts
bun test -t "useDay"

# Run tests in watch mode
bun run test:watch

# Run tests with coverage report
bun run test:coverage
```

## Rust Tests

### Technology Stack
- **Test Framework**: Built-in Rust test framework
- **Async Runtime**: tokio (for async tests)
- **Dependencies**: tempfile (for filesystem tests)

### Test Files

#### `src-tauri/src/timelapse.rs`
Tests for core timelapse functionality:

**Photographer struct:**
- ✓ Creates new Photographer instance
- ✓ Starts and stops timelapse correctly
- ✓ Manages error logs (add, retrieve, clear)
- ✓ Limits error logs to 10,000 entries

**File Management:**
- ✓ Creates day directory if needed (YYYY-MM-DD format)
- ✓ Generates next filename in empty directory (00001.png)
- ✓ Generates next filename with existing files
- ✓ Handles gaps in filename numbering
- ✓ Ignores non-numeric filenames

**Screen Detection:**
- ✓ Detects window overlapping screen (center inside)
- ✓ Detects window not overlapping screen (center outside)
- ✓ Handles edge cases correctly
- ✓ Works with multi-monitor setups

**Error Types:**
- ✓ Error messages display correctly
- ✓ ErrorLogEntry serializes/deserializes properly

#### `src-tauri/src/paths.rs`
- ✓ The library directory name tracks the build profile

#### `src-tauri/src/lib.rs`
Tests for the Tauri command implementations. `tauri::State` wraps a private
reference and has no public constructor, so each state-backed command is a thin
shim over a `*_impl` function that takes `&PhotographerState`; the tests drive
those directly. `evict_old_cache` is split the same
way for a different reason — it takes the library root as a `&Path`, so the
tests can aim them at a `TempDir` instead of `$HOME`.

**greet command:**
- ✓ Returns correct greeting message

**start_timelapse command:**
- ✓ Starts timelapse successfully
- ✓ Returns error when already running

**stop_timelapse command:**
- ✓ Stops timelapse successfully
- ✓ Returns error when not running

**is_timelapse_running command:**
- ✓ Reports correct running status (not running → running → stopped)

**get_error_logs command:**
- ✓ Returns empty logs when running
- ✓ Returns empty logs when not running

**clear_error_logs command:**
- ✓ Clears logs successfully
- ✓ Returns error when not running

**get_screenshot_metadata command:**
- ✓ Returns error when not running
- ✓ Returns None for a frame that was never captured

**evict_old_cache command:** (the only `remove_dir_all` in the app)
- ✓ Removes a cache folder older than 15 days
- ✓ Keeps a cache folder younger than 15 days
- ✓ Puts the cutoff at 15 days, checked an hour either side
- ✓ Skips plain files — only directories are ever removed
- ✓ Counts only the folders it actually removed
- ✓ Reports "does not exist" rather than erroring when there is no cache dir
- ✓ Never reaches outside `.cache`, even for an equally old day folder

Ages are stamped onto the directories with `File::set_times` rather than waited
for, so these tests neither sleep nor depend on when they run.

**frames: protocol:**
- ✓ Serves a screenshot with its MIME type
- ✓ Answers 503 before the frame source is ready, 400 for a malformed path, 404 for a frame that doesn't exist

#### `src-tauri/frame-source/` (`cargo test -p frame_source`)
Needs only ffmpeg, so it runs where the app crate can't build. The integration
tests in `tests/frames.rs` render small videos whose frame N has gray level
`4N`, so every check is against a known frame:
- ✓ Exact frames across chunk and video boundaries
- ✓ A day stitched from converted hours and remaining screenshots, in clock order, with split hours (`--hourly-2`) in part order
- ✓ Screenshots win over an hourly video for the same hour; legacy whole-day videos only when there is nothing else
- ✓ Broken and byte-identical duplicate videos are skipped
- ✓ New screenshots are picked up on the next question
- ✓ The decoded-chunk cache stays under its cap and re-decodes evicted chunks

### Running Rust Tests

```bash
# Run all Rust tests
cd src-tauri
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_photographer_new

# Run tests with coverage (requires cargo-tarpaulin)
cargo tarpaulin --out Html
```

## Test Configuration

### Bun Configuration (`bunfig.toml`)
```toml
[test]
preload = ["./src/test/happydom.ts", "./src/test/setup.ts"]
```

### Test Setup
- `src/test/happydom.ts` registers happy-dom's `window`/`document` globals. It
  loads first because React Testing Library needs them at import time.
- `src/test/setup.ts`:
  - Extends Bun's expect with jest-dom matchers (typed in `src/test/jest-dom.d.ts`)
  - Mocks the Tauri modules (`readDir`, `readFile`, `invoke`) once for the whole
    run. `mock.module` in Bun is process-wide, so test files must not re-mock
    them with a different shape.
  - Mocks URL.createObjectURL and URL.revokeObjectURL
  - Automatic cleanup after each test
- `src/test/mocked.ts` exports `mocked(fn)`, the replacement for `vi.mocked`.

## Key Testing Patterns

### Frontend
- **Mocking Tauri APIs**: All Tauri plugin functions are mocked
- **Hook Testing**: Uses `renderHook` from @testing-library/react
- **Async Testing**: Uses `waitFor` for asynchronous operations
- **State Testing**: Verifies state changes and side effects

### Backend
- **Unit Testing**: Tests individual functions in isolation
- **Integration Testing**: Tests Tauri command interactions with state
- **Filesystem Testing**: Uses temporary directories for safe testing
- **Error Testing**: Verifies error handling and messages

## Continuous Integration

To run all tests in CI:

```bash
# Frontend tests
bun test

# Rust tests
cd src-tauri && cargo test --release
```

## Future Enhancements

Potential areas for additional testing:
1. **E2E Tests**: Add Playwright/Cypress for end-to-end testing
2. **Visual Regression**: Screenshot testing for UI components
3. **Performance Tests**: Benchmark critical paths
4. **Integration Tests**: Test actual screenshot capture on real systems
5. **Accessibility Tests**: Add a11y testing with jest-axe

## Contributing

When adding new features:
1. Write tests for new functionality
2. Ensure all existing tests pass
3. Aim for >80% code coverage
4. Update this document if adding new test files

## Troubleshooting

### Frontend Tests
- If tests fail with "module not found", run `bun install`
- For timeout errors, increase timeout in test configuration
- Clear node_modules and reinstall if seeing weird behavior

### Rust Tests
- Ensure ImageMagick is installed on system
- Some tests require filesystem access
- Use `cargo clean` if seeing compilation errors

## Resources

- [Bun test runner](https://bun.sh/docs/cli/test)
- [React Testing Library](https://testing-library.com/react)
- [Rust Testing Guide](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Tauri Testing](https://tauri.app/develop/tests/)
