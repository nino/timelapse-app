import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { useFolders, useFiles, useVideos } from './useFolders';
import { BaseDirectory } from '@tauri-apps/api/path';

// Mock the Tauri plugin
vi.mock('@tauri-apps/plugin-fs', () => ({
  readDir: vi.fn(),
}));

const { readDir } = await import('@tauri-apps/plugin-fs');

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

    vi.mocked(readDir).mockResolvedValue(mockFolders as any);

    const { result } = renderHook(() => useFolders());

    await waitFor(() => {
      expect(result.current.folders).toEqual(['2025-01-15', '2025-01-16']);
    });

    expect(result.current.foldersError).toBeNull();
    expect(readDir).toHaveBeenCalledWith('Timelapse', {
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

    vi.mocked(readDir).mockResolvedValue(mockEntries as any);

    const { result } = renderHook(() => useFolders());

    await waitFor(() => {
      expect(result.current.folders).toEqual(['folder1', 'folder2']);
    });
  });

  it('should refresh folders when refreshFolders is called', async () => {
    const initialFolders = [
      { name: 'folder1', isDirectory: true, isFile: false },
    ];
    const updatedFolders = [
      { name: 'folder1', isDirectory: true, isFile: false },
      { name: 'folder2', isDirectory: true, isFile: false },
    ];

    vi.mocked(readDir).mockResolvedValueOnce(initialFolders as any);

    const { result } = renderHook(() => useFolders());

    await waitFor(() => {
      expect(result.current.folders).toEqual(['folder1']);
    });

    vi.mocked(readDir).mockResolvedValueOnce(updatedFolders as any);
    result.current.refreshFolders();

    await waitFor(() => {
      expect(result.current.folders).toEqual(['folder1', 'folder2']);
    });

    expect(readDir).toHaveBeenCalledTimes(2);
  });

  it('should clear error on successful refresh', async () => {
    const mockError = new Error('Initial error');
    vi.mocked(readDir).mockRejectedValueOnce(mockError);

    const { result } = renderHook(() => useFolders());

    await waitFor(() => {
      expect(result.current.foldersError).toEqual(mockError);
    });

    const successFolders = [
      { name: 'folder1', isDirectory: true, isFile: false },
    ];
    vi.mocked(readDir).mockResolvedValueOnce(successFolders as any);
    result.current.refreshFolders();

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

    vi.mocked(readDir).mockResolvedValue(mockFiles as any);

    const { result } = renderHook(() => useFiles('2025-01-15'));

    await waitFor(() => {
      expect(result.current.files).toEqual([
        'image1.png',
        'image2.png',
        'image3.png',
      ]);
    });

    expect(result.current.filesError).toBeNull();
    expect(readDir).toHaveBeenCalledWith('Timelapse/2025-01-15', {
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

    await waitFor(() => {
      expect(result.current.filesError).toEqual(mockError);
    });

    expect(result.current.files).toEqual([]);
  });

  it('should filter out directories and only return files', async () => {
    const mockEntries = [
      { name: 'file1.png', isDirectory: false, isFile: true },
      { name: 'subfolder', isDirectory: true, isFile: false },
      { name: 'file2.png', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValue(mockEntries as any);

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

    vi.mocked(readDir).mockResolvedValueOnce(folder1Files as any);

    const { result, rerender } = renderHook(
      ({ folder }) => useFiles(folder),
      { initialProps: { folder: 'folder1' } }
    );

    await waitFor(() => {
      expect(result.current.files).toEqual(['file1.png']);
    });

    vi.mocked(readDir).mockResolvedValueOnce(folder2Files as any);
    rerender({ folder: 'folder2' });

    await waitFor(() => {
      expect(result.current.files).toEqual(['file2.png']);
    });

    expect(readDir).toHaveBeenCalledTimes(2);
    expect(readDir).toHaveBeenNthCalledWith(1, 'Timelapse/folder1', {
      baseDir: BaseDirectory.Home,
    });
    expect(readDir).toHaveBeenNthCalledWith(2, 'Timelapse/folder2', {
      baseDir: BaseDirectory.Home,
    });
  });

  it('should sort files alphabetically', async () => {
    const mockFiles = [
      { name: 'z.png', isDirectory: false, isFile: true },
      { name: 'a.png', isDirectory: false, isFile: true },
      { name: 'm.png', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValue(mockFiles as any);

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

    vi.mocked(readDir).mockResolvedValue(mockEntries as any);

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videos).toEqual([
        '2025-01-16.mov',
        '2025-01-15.mov',
      ]);
    });

    expect(result.current.videosError).toBeNull();
    expect(readDir).toHaveBeenCalledWith('Timelapse', {
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

    vi.mocked(readDir).mockResolvedValue(mockEntries as any);

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videos).toEqual(['video4.mov', 'video1.mov']);
    });
  });

  it('should sort videos in reverse order (most recent first)', async () => {
    const mockEntries = [
      { name: 'a.mov', isDirectory: false, isFile: true },
      { name: 'b.mov', isDirectory: false, isFile: true },
      { name: 'c.mov', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValue(mockEntries as any);

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videos).toEqual(['c.mov', 'b.mov', 'a.mov']);
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

  it('should refresh videos when refreshVideos is called', async () => {
    const initialVideos = [
      { name: 'video1.mov', isDirectory: false, isFile: true },
    ];
    const updatedVideos = [
      { name: 'video1.mov', isDirectory: false, isFile: true },
      { name: 'video2.mov', isDirectory: false, isFile: true },
    ];

    vi.mocked(readDir).mockResolvedValueOnce(initialVideos as any);

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videos).toEqual(['video1.mov']);
    });

    vi.mocked(readDir).mockResolvedValueOnce(updatedVideos as any);
    result.current.refreshVideos();

    await waitFor(() => {
      expect(result.current.videos).toEqual(['video2.mov', 'video1.mov']);
    });

    expect(readDir).toHaveBeenCalledTimes(2);
  });

  it('should clear error on successful refresh', async () => {
    const mockError = new Error('Initial error');
    vi.mocked(readDir).mockRejectedValueOnce(mockError);

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videosError).toEqual(mockError);
    });

    const successVideos = [
      { name: 'video.mov', isDirectory: false, isFile: true },
    ];
    vi.mocked(readDir).mockResolvedValueOnce(successVideos as any);
    result.current.refreshVideos();

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

    vi.mocked(readDir).mockResolvedValue(mockEntries as any);

    const { result } = renderHook(() => useVideos());

    await waitFor(() => {
      expect(result.current.videos).toEqual(['video.mov']);
    });
  });
});
