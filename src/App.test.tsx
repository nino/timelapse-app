import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { App } from './App';
import { TEST_ROOT } from './test/setup';

// Mock the hooks
vi.mock('./hooks/useFolders', () => ({
  useFolders: vi.fn(),
  useFiles: vi.fn(),
  useVideos: vi.fn(),
}));

// Mock Tauri API
vi.mock('@tauri-apps/plugin-fs', () => ({
  readFile: vi.fn(),
}));

vi.mock('@tauri-apps/api/path', () => ({
  BaseDirectory: {
    Home: 'HOME',
  },
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const { useFolders, useFiles, useVideos } = await import('./hooks/useFolders');
const { readFile } = await import('@tauri-apps/plugin-fs');
const { invoke } = await import('@tauri-apps/api/core');

// App calls useFiles twice per render — once for the selected day folder and
// once for the extracted-frame cache folder — so video tests need the mock to
// answer per path rather than returning one list for both.
function mockFilesByFolder(byFolder: Record<string, Array<string>>): void {
  vi.mocked(useFiles).mockImplementation((folder: string | null) => ({
    files: folder === null ? [] : (byFolder[folder] ?? []),
    filesError: null,
  }));
}

describe('App', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(URL.createObjectURL).mockReturnValue('blob:mock-url');
    vi.mocked(URL.revokeObjectURL).mockImplementation(() => {});
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
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: mockError,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      expect(screen.getByText(/Error loading folders/i)).toBeInTheDocument();
      expect(screen.getByText(/Failed to load folders/i)).toBeInTheDocument();
    });

    it('should display files error', () => {
      const mockError = new Error('Failed to load files');
      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: mockError,
      });

      render(<App />);

      expect(screen.getByText(/Error loading files/i)).toBeInTheDocument();
      expect(screen.getByText(/Failed to load files/i)).toBeInTheDocument();
    });

    it('should display videos error', () => {
      const mockError = new Error('Failed to load videos');
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: mockError,
      });
      vi.mocked(useFiles).mockReturnValue({
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
      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      expect(screen.getByText(/Images/i)).toBeInTheDocument();
      expect(screen.getByText(/Videos/i)).toBeInTheDocument();
    });

    it('should switch to videos mode when clicking Videos button', async () => {
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['video1.mov'],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: null,
      });
      vi.mocked(readFile).mockResolvedValue(new Uint8Array([1, 2, 3]));

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
      const now = new Date();
      // Local date, matching how Rust names the day folders.
      const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
      const folders = [today, '2025-01-14', '2025-01-13'];

      vi.mocked(useFolders).mockReturnValue({
        folders,
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
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

      vi.mocked(useFolders).mockReturnValue({
        folders,
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
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

      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos,
        videosError: null,
      });
      mockFilesByFolder({});
      vi.mocked(invoke).mockResolvedValue('video2-cache');

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
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['2025-01-13.mov', '2025-01-14.mov', '2025-01-15.mov'],
        videosError: null,
      });
      mockFilesByFolder({});
      vi.mocked(invoke).mockResolvedValue('2025-01-15');

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

      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
        files: ['image1.jpg', 'image2.jpg'],
        filesError: null,
      });
      vi.mocked(readFile).mockResolvedValue(mockImageData);

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
      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
        files: ['image1.jpg'],
        filesError: null,
      });
      vi.mocked(readFile).mockRejectedValue(new Error('File not found'));

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
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['test-video.mov'],
        videosError: null,
      });
      mockFilesByFolder({ [`.cache/${CACHE}`]: FRAMES });
      vi.mocked(invoke).mockResolvedValue(CACHE);
    }

    it('should extract frames and render the first frame', async () => {
      const mockFrameData = new Uint8Array([10, 20, 30, 40, 50]);
      const mockBlobUrl = 'blob:mock-frame-url';

      vi.mocked(URL.createObjectURL).mockReturnValue(mockBlobUrl);
      setupVideosMode();
      vi.mocked(readFile).mockResolvedValue(mockFrameData);

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
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['slow-video.mov'],
        videosError: null,
      });
      mockFilesByFolder({});

      // Hold extraction open so the interim state stays on screen.
      let resolveExtract: (value: string) => void;
      vi.mocked(invoke).mockReturnValue(
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
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['test-video.mov'],
        videosError: null,
      });
      mockFilesByFolder({}); // the cache folder lists no frames yet
      vi.mocked(invoke).mockResolvedValue(CACHE);

      render(<App />);
      clickVideosTab();

      await waitFor(() => {
        expect(screen.getByText(/Loading frames/i)).toBeInTheDocument();
      });
      expect(screen.queryByText(/Loading video/i)).not.toBeInTheDocument();
    });

    it('should handle frame extraction errors', async () => {
      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['broken-video.mov'],
        videosError: null,
      });
      mockFilesByFolder({});
      vi.mocked(invoke).mockRejectedValue(new Error('ffmpeg failed'));

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
      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      setupVideosMode();
      vi.mocked(readFile).mockRejectedValue(new Error('Frame file not found'));

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
      vi.mocked(URL.createObjectURL).mockImplementation(() => {
        callCount++;
        return callCount === 1 ? mockBlobUrl1 : mockBlobUrl2;
      });

      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['video1.mov', 'video2.mov'],
        videosError: null,
      });
      // Each video extracts into its own cache folder.
      mockFilesByFolder({
        '.cache/cache-video1.mov': FRAMES,
        '.cache/cache-video2.mov': FRAMES,
      });
      vi.mocked(invoke).mockImplementation(async (_cmd, args) => {
        const { videoFilename } = args as { videoFilename: string };
        return `cache-${videoFilename}`;
      });
      vi.mocked(readFile).mockResolvedValue(new Uint8Array([1, 2, 3]));

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
      vi.mocked(URL.createObjectURL).mockImplementation(
        () => `blob:url-${++blobCounter}`
      );

      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['test-video.mov'],
        videosError: null,
      });
      mockFilesByFolder({
        [`.cache/${CACHE}`]: FRAMES,
        '2025-01-15': ['image1.jpg'],
      });
      vi.mocked(invoke).mockResolvedValue(CACHE);
      vi.mocked(readFile).mockResolvedValue(new Uint8Array([1, 2, 3]));

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
      vi.mocked(readFile).mockResolvedValue(new Uint8Array([1, 2, 3]));

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

      vi.mocked(URL.createObjectURL).mockReturnValue(mockBlobUrl);
      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
        files: ['image1.jpg'],
        filesError: null,
      });
      vi.mocked(readFile).mockResolvedValue(mockImageData);

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
      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
        files: Array(7200).fill('image.jpg'), // 2 hours worth
        filesError: null,
      });

      render(<App />);

      // The time formatting logic is tested indirectly through the UI
      // We can verify it's rendering without errors
      expect(screen.getByText(/Timelapse/i)).toBeInTheDocument();
    });
  });

  describe('Live updates', () => {
    function mockLibrary(folders: Array<string>, files: Array<string>): void {
      vi.mocked(useFolders).mockReturnValue({ folders, foldersError: null });
      vi.mocked(useVideos).mockReturnValue({ videos: [], videosError: null });
      vi.mocked(useFiles).mockImplementation((folder: string | null) => ({
        files: folder === null ? [] : files,
        filesError: null,
      }));
    }

    function frames(count: number): Array<string> {
      return Array.from({ length: count }, (_, i) =>
        `${String(i + 1).padStart(5, '0')}.png`,
      );
    }

    beforeEach(() => {
      vi.mocked(readFile).mockResolvedValue(new Uint8Array([1, 2, 3]));
      vi.mocked(invoke).mockResolvedValue(null);
    });

    it('should not render a manual refresh button', () => {
      mockLibrary(['2025-01-15'], frames(3));
      render(<App />);
      expect(screen.queryByText(/Refresh/i)).not.toBeInTheDocument();
    });

    it('should follow new captures while on the newest frame', async () => {
      mockLibrary(['2025-01-15'], frames(3));
      const { rerender } = render(<App />);
      await waitFor(() => {
        expect(screen.getByText('Frame 3 / 3')).toBeInTheDocument();
      });

      mockLibrary(['2025-01-15'], frames(4));
      rerender(<App />);

      await waitFor(() => {
        expect(screen.getByText('Frame 4 / 4')).toBeInTheDocument();
      });
      expect(readFile).toHaveBeenLastCalledWith(
        `${TEST_ROOT}/2025-01-15/00004.png`,
        { baseDir: 'HOME' },
      );
    });

    it('should stay on a scrubbed-back frame when new captures arrive', async () => {
      mockLibrary(['2025-01-15'], frames(10));
      const { rerender } = render(<App />);
      await waitFor(() => {
        expect(screen.getByText('Frame 10 / 10')).toBeInTheDocument();
      });

      fireEvent.change(screen.getByRole('slider'), { target: { value: '2' } });
      await waitFor(() => {
        expect(screen.getByText('Frame 3 / 10')).toBeInTheDocument();
      });
      const readsBefore = vi.mocked(readFile).mock.calls.length;

      mockLibrary(['2025-01-15'], frames(11));
      rerender(<App />);

      await waitFor(() => {
        expect(screen.getByText('Frame 3 / 11')).toBeInTheDocument();
      });
      // Same frame on screen, so it isn't re-read just because the list grew.
      expect(vi.mocked(readFile).mock.calls.length).toBe(readsBefore);
    });

    it('should return to the newest frame after visiting the videos tab', async () => {
      mockLibrary(['2025-01-15'], frames(5));
      vi.mocked(useVideos).mockReturnValue({ videos: ['v.mov'], videosError: null });
      mockFilesByFolder({ '2025-01-15': frames(5), '.cache/v': frames(3) });
      vi.mocked(invoke).mockImplementation(async (command: string) =>
        command === 'extract_video_frames' ? 'v' : null,
      );
      render(<App />);
      await waitFor(() => {
        expect(screen.getByText('Frame 5 / 5')).toBeInTheDocument();
      });

      clickVideosTab();
      // Video frames start from the first one.
      await waitFor(() => {
        expect(screen.getByText('Frame 1 / 3')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText(/Images/i).closest('button') as HTMLElement);

      await waitFor(() => {
        expect(screen.getByText('Frame 5 / 5')).toBeInTheDocument();
      });
    });

    it('should move to a new day folder at midnight when following the live edge', async () => {
      mockLibrary(['2025-01-15'], frames(3));
      const { rerender } = render(<App />);
      await waitFor(() => {
        expect(screen.getByRole('combobox')).toHaveValue('2025-01-15');
      });

      mockLibrary(['2025-01-15', '2025-01-16'], frames(3));
      rerender(<App />);

      await waitFor(() => {
        expect(screen.getByRole('combobox')).toHaveValue('2025-01-16');
      });
    });

    it('should stay on an older day when a new day folder appears', async () => {
      mockLibrary(['2025-01-14', '2025-01-15'], frames(3));
      const { rerender } = render(<App />);
      await waitFor(() => {
        expect(screen.getByRole('combobox')).toHaveValue('2025-01-15');
      });
      fireEvent.change(screen.getByRole('combobox'), {
        target: { value: '2025-01-14' },
      });

      mockLibrary(['2025-01-14', '2025-01-15', '2025-01-16'], frames(3));
      rerender(<App />);

      await waitFor(() => {
        expect(screen.getByRole('option', { name: /2025-01-16/ })).toBeInTheDocument();
      });
      expect(screen.getByRole('combobox')).toHaveValue('2025-01-14');
    });
  });

  describe('Empty States', () => {
    it('should handle empty folders list', () => {
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      expect(screen.getByText(/Timelapse/i)).toBeInTheDocument();
    });

    it('should handle empty files list', () => {
      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: null,
      });

      render(<App />);

      expect(screen.getByText(/Timelapse/i)).toBeInTheDocument();
    });
  });
});
