# Testing Documentation

This document describes the comprehensive test suite for the timelapse-app.

## Overview

The project includes extensive test coverage for both frontend (React/TypeScript) and backend (Rust) code:

- **Frontend Tests**: 34 tests covering React components and hooks
- **Rust Tests**: 23 tests covering core functionality and Tauri commands
- **Total Coverage**: 57 tests across the application

## Frontend Tests

### Technology Stack
- **Test Runner**: Vitest 4.0
- **Testing Library**: @testing-library/react 16.3
- **Environment**: happy-dom (modern, faster alternative to jsdom)
- **Assertions**: @testing-library/jest-dom matchers

### Test Files

#### `src/hooks/useFolders.test.ts` (18 tests)
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
- ✓ Sorts videos in reverse order (most recent first)
- ✓ Handles errors when loading videos
- ✓ Refreshes videos when requested
- ✓ Clears errors on successful refresh
- ✓ Filters out directories

#### `src/App.test.tsx` (16 tests)
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

### Running Frontend Tests

```bash
# Run tests in watch mode (interactive)
yarn test

# Run tests once (CI mode)
yarn test:run

# Run tests with UI
yarn test:ui

# Run tests with coverage report
yarn test:coverage
```

## Rust Tests

### Technology Stack
- **Test Framework**: Built-in Rust test framework
- **Async Runtime**: tokio (for async tests)
- **Dependencies**: tempfile (for filesystem tests)

### Test Files

#### `src-tauri/src/timelapse.rs` (13 tests)
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

#### `src-tauri/src/lib.rs` (10 tests)
Tests for Tauri commands:

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

### Vitest Configuration (`vitest.config.ts`)
```typescript
export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: 'happy-dom',
    setupFiles: ['./src/test/setup.ts'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json', 'html'],
    },
  },
});
```

### Test Setup (`src/test/setup.ts`)
- Extends Vitest expect with jest-dom matchers
- Mocks URL.createObjectURL and URL.revokeObjectURL
- Automatic cleanup after each test

## Test Coverage Summary

| Area | Files | Tests | Coverage |
|------|-------|-------|----------|
| React Hooks | 1 | 18 | ✓ Comprehensive |
| React Components | 1 | 16 | ✓ Comprehensive |
| Rust Core Logic | 1 | 13 | ✓ Comprehensive |
| Rust Tauri Commands | 1 | 10 | ✓ Comprehensive |
| **Total** | **4** | **57** | **✓ Comprehensive** |

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
yarn test:run

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
- If tests fail with "module not found", run `yarn install`
- For timeout errors, increase timeout in test configuration
- Clear node_modules and reinstall if seeing weird behavior

### Rust Tests
- Ensure ImageMagick is installed on system
- Some tests require filesystem access
- Use `cargo clean` if seeing compilation errors

## Resources

- [Vitest Documentation](https://vitest.dev/)
- [React Testing Library](https://testing-library.com/react)
- [Rust Testing Guide](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Tauri Testing](https://tauri.app/develop/tests/)
