import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import { useFolders, useFiles, useVideos } from './useFolders';
import { BaseDirectory } from '@tauri-apps/api/path';
import type { DirEntry } from '@tauri-apps/plugin-fs';
import { TEST_ROOT } from '../test/setup';

// Mock the Tauri plugin
vi.mock('@tauri-apps/plugin-fs', () => ({
  readDir: vi.fn(),
  watch: vi.fn(),
}));

const { readDir, watch } = await import('@tauri-apps/plugin-fs');

// Every watcher the hooks register, keyed by path, so a test can play the part
// of the filesystem and announce a change.
const watchers = new Map<string, () => void>();
const unwatch = vi.fn();

function fireChange(path: string): void {
  const callback = watchers.get(path);
  if (!callback) throw new Error(`Nothing is watching ${path}`);
  act(() => callback());
}

beforeEach(() => {
  watchers.clear();
  vi.mocked(watch).mockImplementation(async (path, callback) => {
    watchers.set(String(path), () => callback({ type: 'any', paths: [String(path)], attrs: null }));
    return unwatch;
  });
});

afterEach(() => {
  vi.useRealTimers();
});

// Test fixtures omit isSymlink; fill it in so mocks satisfy DirEntry without casting.
type MockDirEntry = Omit<DirEntry, 'isSymlink'>;

function dirEntries(entries: Array<MockDirEntry>): Array<DirEntry> {
  return entries.map((entry) => ({ ...entry, isSymlink: false }));
}

describe('useFolders', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should load folders successfully', async () => {
    const mockFolders = [
      { name: '2025-01-15', isDirectory: true, isFile: false },
      { name: '2025-01-16', isDirectory: true, isFile: false },
      { name: 'video.mov', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValue(dirEntries(mockFolders));

    const { result } = renderHook(() => useFolders());

    await waitFor(() => {
      expect(result.current.folders).toEqual(['2025-01-15', '2025-01-16']);
    });

    expect(result.current.foldersError).toBeNull();
    expect(readDir).toHaveBeenCalledWith(TEST_ROOT, {
      baseDir: BaseDirectory.Home,
    });
  });

  it('should handle errors when loading folders', async () => {
    const mockError = new Error('Failed to read directory');
    vi.mocked(readDir).mockRejectedValue(mockError);

    const { result } = renderHook(() => useFolders());

    await waitFor(() => {
      expect(result.current.foldersError).toEqual(mockError);
    });

    expect(result.current.folders).toEqual([]);
  });

  it('should filter out non-directory entries', async () => {
    const mockEntries = [
      { name: 'folder1', isDirectory: true, isFile: false },
      { name: 'file.txt', isDirectory: false, isFile: true },
      { name: 'folder2', isDirectory: true, isFile: false },
    ];

    vi.mocked(readDir).mockResolvedValue(dirEntries(mockEntries));

    const { result } = renderHook(() => useFolders());

    await waitFor(() => {
      expect(result.current.folders).toEqual(['folder1', 'folder2']);
    });
  });

  it('should reload folders when the library root changes on disk', async () => {
    const initialFolders = [
      { name: 'folder1', isDirectory: true, isFile: false },
    ];
    const updatedFolders = [
      { name: 'folder1', isDirectory: true, isFile: false },
      { name: 'folder2', isDirectory: true, isFile: false },
    ];

    vi.mocked(readDir).mockResolvedValueOnce(dirEntries(initialFolders));

    const { result } = renderHook(() => useFolders());

    await waitFor(() => {
      expect(result.current.folders).toEqual(['folder1']);
    });

    vi.mocked(readDir).mockResolvedValueOnce(dirEntries(updatedFolders));
    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));
    fireChange(TEST_ROOT);

    await waitFor(() => {
      expect(result.current.folders).toEqual(['folder1', 'folder2']);
    });

    expect(readDir).toHaveBeenCalledTimes(2);
  });

  it('should clear error on a successful reload', async () => {
    const mockError = new Error('Initial error');
    vi.mocked(readDir).mockRejectedValueOnce(mockError);

    const { result } = renderHook(() => useFolders());

    await waitFor(() => {
      expect(result.current.foldersError).toEqual(mockError);
    });

    const successFolders = [
      { name: 'folder1', isDirectory: true, isFile: false },
    ];
    vi.mocked(readDir).mockResolvedValueOnce(dirEntries(successFolders));
    fireChange(TEST_ROOT);

    await waitFor(() => {
      expect(result.current.foldersError).toBeNull();
      expect(result.current.folders).toEqual(['folder1']);
    });
  });
});

describe('useFiles', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should load files successfully when folder is provided', async () => {
    const mockFiles = [
      { name: 'image3.png', isDirectory: false, isFile: true },
      { name: 'image1.png', isDirectory: false, isFile: true },
      { name: 'image2.png', isDirectory: false, isFile: true },
      { name: 'subfolder', isDirectory: true, isFile: false },
    ];

    vi.mocked(readDir).mockResolvedValue(dirEntries(mockFiles));

    const { result } = renderHook(() => useFiles('2025-01-15'));

    await waitFor(() => {
      expect(result.current.files).toEqual([
        'image1.png',
        'image2.png',
        'image3.png',
      ]);
    });

    expect(result.current.filesError).toBeNull();
    expect(readDir).toHaveBeenCalledWith(`${TEST_ROOT}/2025-01-15`, {
      baseDir: BaseDirectory.Home,
    });
  });

  it('should return empty array when folder is null', async () => {
    const { result } = renderHook(() => useFiles(null));

    await waitFor(() => {
      expect(result.current.files).toEqual([]);
    });

    expect(readDir).not.toHaveBeenCalled();
    expect(result.current.filesError).toBeNull();
  });

  it('should handle errors when loading files', async () => {
    const mockError = new Error('Failed to read files');
    vi.mocked(readDir).mockRejectedValue(mockError);

    const { result } = renderHook(() => useFiles('2025-01-15'));

    // A date folder is not retried, so the error surfaces on the first attempt.
    // The default 1s waitFor timeout is the assertion: the ~2s cache-folder
    // retry loop must not run here.
    await waitFor(() => {
      expect(result.current.filesError).toEqual(mockError);
    });

    expect(result.current.files).toEqual([]);
    expect(readDir).toHaveBeenCalledTimes(1);
  });

  it('should retry a cache folder before surfacing an error', async () => {
    const mockError = new Error('Failed to read files');
    vi.mocked(readDir).mockRejectedValue(mockError);

    const { result } = renderHook(() => useFiles('.cache/2025-01-15'));

    await waitFor(
      () => {
        expect(result.current.filesError).toEqual(mockError);
      },
      { timeout: 5000 },
    );

    // Frames appear asynchronously while ffmpeg writes, so a cache folder is
    // worth retrying — five attempts before giving up.
    expect(readDir).toHaveBeenCalledTimes(5);
  });

  it('should filter out directories and only return files', async () => {
    const mockEntries = [
      { name: 'file1.png', isDirectory: false, isFile: true },
      { name: 'subfolder', isDirectory: true, isFile: false },
      { name: 'file2.png', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValue(dirEntries(mockEntries));

    const { result } = renderHook(() => useFiles('test-folder'));

    await waitFor(() => {
      expect(result.current.files).toEqual(['file1.png', 'file2.png']);
    });
  });

  it('should reload files when folder changes', async () => {
    const folder1Files = [
      { name: 'file1.png', isDirectory: false, isFile: true },
    ];
    const folder2Files = [
      { name: 'file2.png', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValueOnce(dirEntries(folder1Files));

    const { result, rerender } = renderHook(
      ({ folder }) => useFiles(folder),
      { initialProps: { folder: 'folder1' } }
    );

    await waitFor(() => {
      expect(result.current.files).toEqual(['file1.png']);
    });

    vi.mocked(readDir).mockResolvedValueOnce(dirEntries(folder2Files));
    rerender({ folder: 'folder2' });

    await waitFor(() => {
      expect(result.current.files).toEqual(['file2.png']);
    });

    expect(readDir).toHaveBeenCalledTimes(2);
    expect(readDir).toHaveBeenNthCalledWith(1, `${TEST_ROOT}/folder1`, {
      baseDir: BaseDirectory.Home,
    });
    expect(readDir).toHaveBeenNthCalledWith(2, `${TEST_ROOT}/folder2`, {
      baseDir: BaseDirectory.Home,
    });
  });

  it('should sort files alphabetically', async () => {
    const mockFiles = [
      { name: 'z.png', isDirectory: false, isFile: true },
      { name: 'a.png', isDirectory: false, isFile: true },
      { name: 'm.png', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValue(dirEntries(mockFiles));

    const { result } = renderHook(() => useFiles('test'));

    await waitFor(() => {
      expect(result.current.files).toEqual(['a.png', 'm.png', 'z.png']);
    });
  });
});

describe('useVideos', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should load video files successfully', async () => {
    const mockEntries = [
      { name: '2025-01-15.mov', isDirectory: false, isFile: true },
      { name: '2025-01-16.mov', isDirectory: false, isFile: true },
      { name: 'folder', isDirectory: true, isFile: false },
      { name: 'image.png', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValue(dirEntries(mockEntries));

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videos).toEqual([
        '2025-01-15.mov',
        '2025-01-16.mov',
      ]);
    });

    expect(result.current.videosError).toBeNull();
    expect(readDir).toHaveBeenCalledWith(TEST_ROOT, {
      baseDir: BaseDirectory.Home,
    });
  });

  it('should only include .mov files', async () => {
    const mockEntries = [
      { name: 'video1.mov', isDirectory: false, isFile: true },
      { name: 'video2.mp4', isDirectory: false, isFile: true },
      { name: 'video3.avi', isDirectory: false, isFile: true },
      { name: 'video4.mov', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValue(dirEntries(mockEntries));

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videos).toEqual(['video1.mov', 'video4.mov']);
    });
  });

  it('should sort videos chronologically (oldest first)', async () => {
    const mockEntries = [
      { name: 'a.mov', isDirectory: false, isFile: true },
      { name: 'b.mov', isDirectory: false, isFile: true },
      { name: 'c.mov', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValue(dirEntries(mockEntries));

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videos).toEqual(['a.mov', 'b.mov', 'c.mov']);
    });
  });

  it('should handle errors when loading videos', async () => {
    const mockError = new Error('Failed to read videos');
    vi.mocked(readDir).mockRejectedValue(mockError);

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videosError).toEqual(mockError);
    });

    expect(result.current.videos).toEqual([]);
  });

  it('should reload videos when the library root changes on disk', async () => {
    const initialVideos = [
      { name: 'video1.mov', isDirectory: false, isFile: true },
    ];
    const updatedVideos = [
      { name: 'video1.mov', isDirectory: false, isFile: true },
      { name: 'video2.mov', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValueOnce(dirEntries(initialVideos));

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videos).toEqual(['video1.mov']);
    });

    vi.mocked(readDir).mockResolvedValueOnce(dirEntries(updatedVideos));
    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));
    fireChange(TEST_ROOT);

    await waitFor(() => {
      expect(result.current.videos).toEqual(['video1.mov', 'video2.mov']);
    });

    expect(readDir).toHaveBeenCalledTimes(2);
  });

  it('should clear error on a successful reload', async () => {
    const mockError = new Error('Initial error');
    vi.mocked(readDir).mockRejectedValueOnce(mockError);

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videosError).toEqual(mockError);
    });

    const successVideos = [
      { name: 'video.mov', isDirectory: false, isFile: true },
    ];
    vi.mocked(readDir).mockResolvedValueOnce(dirEntries(successVideos));
    fireChange(TEST_ROOT);

    await waitFor(() => {
      expect(result.current.videosError).toBeNull();
      expect(result.current.videos).toEqual(['video.mov']);
    });
  });

  it('should filter out directories', async () => {
    const mockEntries = [
      { name: 'video.mov', isDirectory: false, isFile: true },
      { name: 'folder.mov', isDirectory: true, isFile: false },
    ];

    vi.mocked(readDir).mockResolvedValue(dirEntries(mockEntries));

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videos).toEqual(['video.mov']);
    });
  });
});

describe('live updates', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should pick up new screenshots in a date folder as they are written', async () => {
    const folder = `${TEST_ROOT}/2025-01-15`;
    vi.mocked(readDir).mockResolvedValueOnce(
      dirEntries([{ name: '00001.png', isDirectory: false, isFile: true }]),
    );

    const { result } = renderHook(() => useFiles('2025-01-15'));
    await waitFor(() => expect(result.current.files).toEqual(['00001.png']));

    vi.mocked(readDir).mockResolvedValueOnce(
      dirEntries([
        { name: '00001.png', isDirectory: false, isFile: true },
        { name: '00002.png', isDirectory: false, isFile: true },
      ]),
    );
    await waitFor(() => expect(watchers.has(folder)).toBe(true));
    fireChange(folder);

    await waitFor(() =>
      expect(result.current.files).toEqual(['00001.png', '00002.png']),
    );
    expect(watch).toHaveBeenCalledWith(folder, expect.any(Function), {
      baseDir: BaseDirectory.Home,
      delayMs: 500,
    });
  });

  it('should not watch a published cache folder', async () => {
    vi.mocked(readDir).mockResolvedValue(
      dirEntries([{ name: 'frame000001.jpg', isDirectory: false, isFile: true }]),
    );

    const { result } = renderHook(() => useFiles('.cache/video'));
    await waitFor(() => expect(result.current.files).toEqual(['frame000001.jpg']));

    expect(watch).not.toHaveBeenCalled();
  });

  it('should keep the same array when a reload finds nothing new', async () => {
    const entries = dirEntries([{ name: '2025-01-15', isDirectory: true, isFile: false }]);
    vi.mocked(readDir).mockResolvedValue(entries);

    const { result } = renderHook(() => useFolders());
    await waitFor(() => expect(result.current.folders).toEqual(['2025-01-15']));
    const first = result.current.folders;

    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));
    fireChange(TEST_ROOT);
    await waitFor(() => expect(readDir).toHaveBeenCalledTimes(2));

    expect(result.current.folders).toBe(first);
  });

  it('should not show the previous folder\'s files while a new folder loads', async () => {
    vi.mocked(readDir).mockResolvedValueOnce(
      dirEntries([{ name: 'old.png', isDirectory: false, isFile: true }]),
    );
    let resolveNew: (entries: Array<DirEntry>) => void = () => {};
    vi.mocked(readDir).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveNew = resolve;
      }),
    );

    const { result, rerender } = renderHook(
      ({ folder }: { folder: string }) => useFiles(folder),
      { initialProps: { folder: '2025-01-14' } },
    );
    await waitFor(() => expect(result.current.files).toEqual(['old.png']));

    rerender({ folder: '2025-01-15' });
    expect(result.current.files).toEqual([]);

    await act(async () => {
      resolveNew(dirEntries([{ name: 'new.png', isDirectory: false, isFile: true }]));
    });
    expect(result.current.files).toEqual(['new.png']);
  });

  it('should ignore a slow listing for a folder that is no longer selected', async () => {
    let resolveOld: (entries: Array<DirEntry>) => void = () => {};
    vi.mocked(readDir).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      }),
    );
    vi.mocked(readDir).mockResolvedValueOnce(
      dirEntries([{ name: 'new.png', isDirectory: false, isFile: true }]),
    );

    const { result, rerender } = renderHook(
      ({ folder }: { folder: string }) => useFiles(folder),
      { initialProps: { folder: '2025-01-14' } },
    );
    rerender({ folder: '2025-01-15' });
    await waitFor(() => expect(result.current.files).toEqual(['new.png']));

    await act(async () => {
      resolveOld(dirEntries([{ name: 'old.png', isDirectory: false, isFile: true }]));
    });
    expect(result.current.files).toEqual(['new.png']);

    // Switching back must not resurrect the stale listing either.
    vi.mocked(readDir).mockResolvedValueOnce(
      dirEntries([{ name: 'fresh.png', isDirectory: false, isFile: true }]),
    );
    rerender({ folder: '2025-01-14' });
    await waitFor(() => expect(result.current.files).toEqual(['fresh.png']));
  });

  it('should stop watching on unmount', async () => {
    vi.mocked(readDir).mockResolvedValue([]);
    const { unmount } = renderHook(() => useFolders());
    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));

    unmount();

    expect(unwatch).toHaveBeenCalled();
  });

  it('should fall back to polling when the watcher cannot be created', async () => {
    vi.useFakeTimers();
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.mocked(watch).mockRejectedValue(new Error('fs.watch not allowed'));
    vi.mocked(readDir).mockResolvedValue([]);

    renderHook(() => useVideos());
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(readDir).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(readDir).toHaveBeenCalledTimes(2);
  });
});
