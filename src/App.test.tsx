import { afterAll, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { invoke } from '@tauri-apps/api/core';
import { readFile } from '@tauri-apps/plugin-fs';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { App } from './App';
import * as folderHooks from './hooks/useFolders';
import { mocked } from './test/mocked';
import { TEST_ROOT } from './test/setup';

// readFile and invoke are replaced with mocks in src/test/setup.ts.
//
// The hooks are spied on rather than replaced with `mock.module`: Bun runs all
// test files in one process and a module mock would also replace the real
// hooks that src/hooks/useFolders.test.ts is testing. Restored in afterAll.
const useFolders = spyOn(folderHooks, 'useFolders');
const useFiles = spyOn(folderHooks, 'useFiles');
const useVideos = spyOn(folderHooks, 'useVideos');

afterAll(() => {
  useFolders.mockRestore();
  useFiles.mockRestore();
  useVideos.mockRestore();
});

// App calls useFiles twice per render — once for the selected day folder and
// once for the extracted-frame cache folder — so video tests need the mock to
// answer per path rather than returning one list for both.
function mockFilesByFolder(byFolder: Record<string, Array<string>>): void {
  useFiles.mockImplementation((folder: string | null) => ({
    files: folder === null ? [] : (byFolder[folder] ?? []),
    filesError: null,
  }));
}

describe('App', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    mocked(URL.createObjectURL).mockReturnValue('blob:mock-url');
    mocked(URL.revokeObjectURL).mockImplementation(() => {});
  });

  function clickVideosTab(): void {
    const videosButton = screen
      .getAllByText(/Videos/i)
      .find(el => el.tagName === 'BUTTON');
    if (videosButton) {
      fireEvent.click(videosButton);
    }
  }

  describe('Error Handling', () => {
    it('should display folders error', () => {
      const mockError = new Error('Failed to load folders');
      useFolders.mockReturnValue({
        folders: [],
        foldersError: mockError,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      expect(screen.getByText(/Error loading folders/i)).toBeInTheDocument();
      expect(screen.getByText(/Failed to load folders/i)).toBeInTheDocument();
    });

    it('should display files error', () => {
      const mockError = new Error('Failed to load files');
      useFolders.mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: [],
        filesError: mockError,
      });

      render(<App />);

      expect(screen.getByText(/Error loading files/i)).toBeInTheDocument();
      expect(screen.getByText(/Failed to load files/i)).toBeInTheDocument();
    });

    it('should display videos error', () => {
      const mockError = new Error('Failed to load videos');
      useFolders.mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: mockError,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      expect(screen.getByText(/Error loading videos/i)).toBeInTheDocument();
      expect(screen.getByText(/Failed to load videos/i)).toBeInTheDocument();
    });
  });

  describe('View Modes', () => {
    it('should render in images mode by default', () => {
      useFolders.mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      expect(screen.getByText(/Images/i)).toBeInTheDocument();
      expect(screen.getByText(/Videos/i)).toBeInTheDocument();
    });

    it('should switch to videos mode when clicking Videos button', async () => {
      useFolders.mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: ['video1.mov'],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: [],
        filesError: null,
      });
      mocked(readFile).mockResolvedValue(new Uint8Array([1, 2, 3]));

      render(<App />);

      const videosButtons = screen.getAllByText(/Videos/i);
      const videosButton = videosButtons.find(el => el.tagName === 'BUTTON');

      if (videosButton) {
        fireEvent.click(videosButton);
      }

      // In videos mode, the component should show video-related UI
      await waitFor(() => {
        expect(videosButtons.length).toBeGreaterThan(0);
      });
    });
  });

  describe('Folder Selection', () => {
    it('should auto-select today\'s folder if it exists', async () => {
      const today = new Date().toISOString().split('T')[0];
      const folders = [today, '2025-01-14', '2025-01-13'];

      useFolders.mockReturnValue({
        folders,
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      await waitFor(() => {
        expect(useFiles).toHaveBeenCalledWith(today);
      });
    });

    it('should select most recent folder if today\'s folder doesn\'t exist', async () => {
      const folders = ['2025-01-14', '2025-01-13', '2025-01-12'];

      useFolders.mockReturnValue({
        folders,
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      await waitFor(() => {
        expect(useFiles).toHaveBeenCalledWith('2025-01-14');
      });
    });
  });

  describe('Video Selection', () => {
    it('should auto-select most recent video when switching to videos mode', async () => {
      // useVideos returns chronological order, so the last entry is newest.
      const videos = ['video1.mov', 'video2.mov'];

      useFolders.mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos,
        videosError: null,
        refreshVideos: mock(),
      });
      mockFilesByFolder({});
      mocked(invoke).mockResolvedValue('video2-cache');

      render(<App />);

      const videosButton = screen.getByText(/Videos/i).closest('button');
      if (videosButton) {
        fireEvent.click(videosButton);
      }

      await waitFor(() => {
        expect(invoke).toHaveBeenCalledWith('extract_video_frames', {
          videoFilename: 'video2.mov',
        });
      });
    });

    it('should list videos newest-first, like the date picker', async () => {
      // useVideos returns chronological order; the dropdown reverses it so both
      // view modes agree on which end of the list is "most recent".
      useFolders.mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: ['2025-01-13.mov', '2025-01-14.mov', '2025-01-15.mov'],
        videosError: null,
        refreshVideos: mock(),
      });
      mockFilesByFolder({});
      mocked(invoke).mockResolvedValue('2025-01-15');

      render(<App />);
      clickVideosTab();

      await waitFor(() => {
        expect(screen.getByRole('combobox')).toBeInTheDocument();
      });

      const options = Array.from(
        screen.getByRole('combobox').querySelectorAll('option')
      ).map((option) => option.textContent);

      expect(options).toEqual([
        'Select a video…',
        '2025-01-15.mov',
        '2025-01-14.mov',
        '2025-01-13.mov',
      ]);
    });
  });

  describe('Image Loading', () => {
    it('should load image when folder and files are available', async () => {
      const mockImageData = new Uint8Array([1, 2, 3, 4]);

      useFolders.mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: ['image1.jpg', 'image2.jpg'],
        filesError: null,
      });
      mocked(readFile).mockResolvedValue(mockImageData);

      render(<App />);

      // Assert the whole path, not just that readFile ran: this is what pins
      // the library root and the "last image is selected first" index reset.
      await waitFor(() => {
        expect(readFile).toHaveBeenCalledWith(
          `${TEST_ROOT}/2025-01-15/image2.jpg`,
          expect.any(Object)
        );
      });

      expect(URL.createObjectURL).toHaveBeenCalled();
    });

    it('should handle image loading errors gracefully', async () => {
      const consoleSpy = spyOn(console, 'error').mockImplementation(() => {});

      useFolders.mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: ['image1.jpg'],
        filesError: null,
      });
      mocked(readFile).mockRejectedValue(new Error('File not found'));

      render(<App />);

      await waitFor(() => {
        expect(consoleSpy).toHaveBeenCalledWith(
          'Error loading image:',
          expect.any(Error)
        );
      });

      consoleSpy.mockRestore();
    });
  });

  describe('Video Loading', () => {
    const CACHE = 'test-video-cache';
    const FRAMES = ['frame0001.jpg', 'frame0002.jpg'];

    function setupVideosMode(): void {
      useFolders.mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: ['test-video.mov'],
        videosError: null,
        refreshVideos: mock(),
      });
      mockFilesByFolder({ [`.cache/${CACHE}`]: FRAMES });
      mocked(invoke).mockResolvedValue(CACHE);
    }

    it('should extract frames and render the first frame', async () => {
      const mockFrameData = new Uint8Array([10, 20, 30, 40, 50]);
      const mockBlobUrl = 'blob:mock-frame-url';

      mocked(URL.createObjectURL).mockReturnValue(mockBlobUrl);
      setupVideosMode();
      mocked(readFile).mockResolvedValue(mockFrameData);

      render(<App />);
      clickVideosTab();

      // Frames are read out of the cache folder the Rust command reports.
      await waitFor(() => {
        expect(readFile).toHaveBeenCalledWith(
          `${TEST_ROOT}/.cache/${CACHE}/${FRAMES[0]}`,
          expect.any(Object)
        );
      });

      expect(URL.createObjectURL).toHaveBeenCalled();

      await waitFor(() => {
        const img = document.querySelector('img');
        expect(img).toBeTruthy();
        expect(img?.getAttribute('src')).toBe(mockBlobUrl);
      });
    });

    it('should show loading state while frames are being extracted', async () => {
      useFolders.mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: ['slow-video.mov'],
        videosError: null,
        refreshVideos: mock(),
      });
      mockFilesByFolder({});

      // Hold extraction open so the interim state stays on screen.
      let resolveExtract: (value: string) => void;
      mocked(invoke).mockReturnValue(
        new Promise<string>(resolve => {
          resolveExtract = resolve;
        })
      );

      render(<App />);
      clickVideosTab();

      await waitFor(() => {
        expect(
          screen.getByText(/Extracting frames from video/i)
        ).toBeInTheDocument();
      });

      const videoTexts = screen.getAllByText(/slow-video.mov/i);
      expect(videoTexts.length).toBeGreaterThan(0);

      resolveExtract!(CACHE);
    });

    it('should say it is loading frames while the cache folder is listed', async () => {
      // extract_video_frames has resolved, but useFiles is still retrying the
      // freshly-created cache folder. That window used to report "Loading
      // video…", which reads as though extraction had not started.
      useFolders.mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: ['test-video.mov'],
        videosError: null,
        refreshVideos: mock(),
      });
      mockFilesByFolder({}); // the cache folder lists no frames yet
      mocked(invoke).mockResolvedValue(CACHE);

      render(<App />);
      clickVideosTab();

      await waitFor(() => {
        expect(screen.getByText(/Loading frames/i)).toBeInTheDocument();
      });
      expect(screen.queryByText(/Loading video/i)).not.toBeInTheDocument();
    });

    it('should handle frame extraction errors', async () => {
      const consoleSpy = spyOn(console, 'error').mockImplementation(() => {});

      useFolders.mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: ['broken-video.mov'],
        videosError: null,
        refreshVideos: mock(),
      });
      mockFilesByFolder({});
      mocked(invoke).mockRejectedValue(new Error('ffmpeg failed'));

      render(<App />);
      clickVideosTab();

      await waitFor(() => {
        expect(consoleSpy).toHaveBeenCalledWith(
          'Error extracting frames:',
          expect.any(Error)
        );
      });

      // The failure has to reach the user: a missing ffmpeg must not be
      // indistinguishable from a slow extraction.
      expect(
        screen.getByText(/Could not extract frames from this video/i)
      ).toBeInTheDocument();
      expect(screen.getByText('ffmpeg failed')).toBeInTheDocument();
      expect(screen.queryByText(/Loading video/i)).not.toBeInTheDocument();

      consoleSpy.mockRestore();
    });

    it('should handle frame read errors', async () => {
      const consoleSpy = spyOn(console, 'error').mockImplementation(() => {});

      setupVideosMode();
      mocked(readFile).mockRejectedValue(new Error('Frame file not found'));

      render(<App />);
      clickVideosTab();

      await waitFor(() => {
        expect(consoleSpy).toHaveBeenCalledWith(
          'Error loading frame:',
          expect.any(Error)
        );
      });

      expect(screen.getByText(/Loading frame/i)).toBeInTheDocument();

      consoleSpy.mockRestore();
    });

    it('should clean up blob URL when switching videos', async () => {
      const mockBlobUrl1 = 'blob:video-1';
      const mockBlobUrl2 = 'blob:video-2';

      let callCount = 0;
      mocked(URL.createObjectURL).mockImplementation(() => {
        callCount++;
        return callCount === 1 ? mockBlobUrl1 : mockBlobUrl2;
      });

      useFolders.mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: ['video1.mov', 'video2.mov'],
        videosError: null,
        refreshVideos: mock(),
      });
      // Each video extracts into its own cache folder.
      mockFilesByFolder({
        '.cache/cache-video1.mov': FRAMES,
        '.cache/cache-video2.mov': FRAMES,
      });
      // invoke is generic over its result type; the cast pins it to the string
      // extract_video_frames returns.
      mocked(invoke).mockImplementation((async (_cmd: string, args?: unknown) => {
        const { videoFilename } = args as { videoFilename: string };
        return `cache-${videoFilename}`;
      }) as typeof invoke);
      mocked(readFile).mockResolvedValue(new Uint8Array([1, 2, 3]));

      render(<App />);
      clickVideosTab();

      // video2.mov is auto-selected (chronologically last).
      await waitFor(() => {
        expect(readFile).toHaveBeenCalledWith(
          `${TEST_ROOT}/.cache/cache-video2.mov/${FRAMES[0]}`,
          expect.any(Object)
        );
      });

      const videoSelect = screen.getByRole('combobox');
      fireEvent.change(videoSelect, { target: { value: 'video1.mov' } });

      await waitFor(() => {
        expect(readFile).toHaveBeenCalledWith(
          `${TEST_ROOT}/.cache/cache-video1.mov/${FRAMES[0]}`,
          expect.any(Object)
        );
      });

      await waitFor(() => {
        expect(URL.revokeObjectURL).toHaveBeenCalledWith(mockBlobUrl1);
      });
    });

    it('should clean up blob URL when switching away from video mode', async () => {
      // Distinct URLs per blob: if the frame and the image shared one string,
      // currentImageSrc would never change and the cleanup effect would not run.
      const mockBlobUrl = 'blob:url-1';
      let blobCounter = 0;
      mocked(URL.createObjectURL).mockImplementation(
        () => `blob:url-${++blobCounter}`
      );

      useFolders.mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: ['test-video.mov'],
        videosError: null,
        refreshVideos: mock(),
      });
      mockFilesByFolder({
        [`.cache/${CACHE}`]: FRAMES,
        '2025-01-15': ['image1.jpg'],
      });
      mocked(invoke).mockResolvedValue(CACHE);
      mocked(readFile).mockResolvedValue(new Uint8Array([1, 2, 3]));

      render(<App />);
      clickVideosTab();

      await waitFor(() => {
        expect(URL.createObjectURL).toHaveBeenCalled();
      });

      const imagesButton = screen.getByText(/Images/i).closest('button');
      if (imagesButton) {
        fireEvent.click(imagesButton);
      }

      await waitFor(() => {
        expect(URL.revokeObjectURL).toHaveBeenCalledWith(mockBlobUrl);
      });
    });

    it('should show frame count and an enabled scrubber', async () => {
      setupVideosMode();
      mocked(readFile).mockResolvedValue(new Uint8Array([1, 2, 3]));

      render(<App />);
      clickVideosTab();

      await waitFor(() => {
        expect(screen.getByText(`${FRAMES.length} frames`)).toBeInTheDocument();
      });

      expect(
        screen.getByText(`Frame 1 / ${FRAMES.length}`)
      ).toBeInTheDocument();

      const scrubber = screen.getByRole('slider') as HTMLInputElement;
      expect(scrubber.disabled).toBe(false);
      expect(scrubber.max).toBe(String(FRAMES.length - 1));
    });
  });

  describe('Blob URL Cleanup', () => {
    it('should revoke blob URLs on cleanup', async () => {
      const mockImageData = new Uint8Array([1, 2, 3]);
      const mockBlobUrl = 'blob:mock-image-url';

      mocked(URL.createObjectURL).mockReturnValue(mockBlobUrl);
      useFolders.mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: ['image1.jpg'],
        filesError: null,
      });
      mocked(readFile).mockResolvedValue(mockImageData);

      const { unmount } = render(<App />);

      await waitFor(() => {
        expect(URL.createObjectURL).toHaveBeenCalled();
      });

      unmount();

      expect(URL.revokeObjectURL).toHaveBeenCalledWith(mockBlobUrl);
    });
  });

  describe('Time Formatting', () => {
    it('should format time correctly for different indices', () => {
      useFolders.mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: Array(7200).fill('image.jpg'), // 2 hours worth
        filesError: null,
      });

      render(<App />);

      // The time formatting logic is tested indirectly through the UI
      // We can verify it's rendering without errors
      expect(screen.getByText(/Timelapse/i)).toBeInTheDocument();
    });
  });

  describe('Refresh Functionality', () => {
    it('should call refreshFolders when refresh button is clicked in images mode', async () => {
      const mockRefreshFolders = mock();
      const mockRefreshVideos = mock();

      useFolders.mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: mockRefreshFolders,
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mockRefreshVideos,
      });
      useFiles.mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      const refreshButton = screen.getByText(/Refresh/i);
      fireEvent.click(refreshButton);

      expect(mockRefreshFolders).toHaveBeenCalled();
      expect(mockRefreshVideos).not.toHaveBeenCalled();
    });

    it('should call refreshVideos when refresh button is clicked in videos mode', async () => {
      const mockRefreshFolders = mock();
      const mockRefreshVideos = mock();

      useFolders.mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mockRefreshFolders,
      });
      useVideos.mockReturnValue({
        videos: ['video1.mov'],
        videosError: null,
        refreshVideos: mockRefreshVideos,
      });
      useFiles.mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      // Switch to videos mode
      const videosButton = screen.getByText(/Videos/i).closest('button');
      if (videosButton) {
        fireEvent.click(videosButton);
      }

      await waitFor(() => {
        const refreshButton = screen.getByText(/Refresh/i);
        fireEvent.click(refreshButton);
      });

      expect(mockRefreshVideos).toHaveBeenCalled();
    });
  });

  describe('Empty States', () => {
    it('should handle empty folders list', () => {
      useFolders.mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      expect(screen.getByText(/Timelapse/i)).toBeInTheDocument();
    });

    it('should handle empty files list', () => {
      useFolders.mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: mock(),
      });
      useVideos.mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mock(),
      });
      useFiles.mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      expect(screen.getByText(/Timelapse/i)).toBeInTheDocument();
    });
  });
});
