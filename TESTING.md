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

#### `src/hooks/useFolders.test.ts`
Tests for custom React hooks that handle file system operations:

**useFolders hook:**
- ✓ Loads folders successfully from Timelapse directory
- ✓ Handles errors when loading folders
- ✓ Filters out non-directory entries
- ✓ Refreshes folders when requested
- ✓ Clears errors on successful refresh

**useFiles hook:**
- ✓ Loads files successfully when folder is provided
- ✓ Returns empty array when folder is null
- ✓ Handles errors when loading files
- ✓ Filters out directories, only returns files
- ✓ Reloads files when folder changes
- ✓ Sorts files alphabetically

**useVideos hook:**
- ✓ Loads video files (.mov) successfully
- ✓ Only includes .mov files (filters other formats)
- ✓ Sorts videos chronologically (oldest first)
- ✓ Handles errors when loading videos
- ✓ Refreshes videos when requested
- ✓ Clears errors on successful refresh
- ✓ Filters out directories

#### `src/App.test.tsx`
Tests for the main App component:

**Error Handling:**
- ✓ Displays folders error
- ✓ Displays files error
- ✓ Displays videos error

**View Modes:**
- ✓ Renders in images mode by default
- ✓ Switches to videos mode when clicking Videos button

**Folder Selection:**
- ✓ Auto-selects today's folder if it exists
- ✓ Selects most recent folder if today's doesn't exist

**Video Selection:**
- ✓ Auto-selects most recent video when switching to videos mode

**Image Loading:**
- ✓ Loads image when folder and files are available
- ✓ Handles image loading errors gracefully

**Video Loading:**
- ✓ Extracts frames and renders the first frame
- ✓ Shows loading state while frames are being extracted
- ✓ Handles frame extraction errors
- ✓ Handles frame read errors
- ✓ Cleans up blob URL when switching videos
- ✓ Cleans up blob URL when switching away from video mode
- ✓ Shows frame count and an enabled scrubber

**Blob URL Cleanup:**
- ✓ Revokes blob URLs on cleanup (prevents memory leaks)

**Time Formatting:**
- ✓ Formats time correctly for different indices

**Refresh Functionality:**
- ✓ Calls refreshFolders when refresh button clicked (images mode)
- ✓ Calls refreshVideos when refresh button clicked (videos mode)

**Empty States:**
- ✓ Handles empty folders list
- ✓ Handles empty files list

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
bun test src/hooks/useFolders.test.ts
bun test -t "useVideos"

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
those directly. `evict_old_cache` and `extract_video_frames` are split the same
way for a different reason — theirs take the library root as a `&Path`, so the
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

**extract_video_frames command:**
- ✓ Reuses a populated cache folder without invoking ffmpeg
- ✓ Treats an *empty* cache folder as a miss, not a hit
- ✓ Creates the cache folder before invoking ffmpeg

Only the first of these needs the cache-hit branch; the other two fall through
to ffmpeg and assert on the failure, which holds whether or not ffmpeg is
installed.

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
