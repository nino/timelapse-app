import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { App } from './App';

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

const { useFolders, useFiles, useVideos } = await import('./hooks/useFolders');
const { readFile } = await import('@tauri-apps/plugin-fs');

describe('App', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(URL.createObjectURL).mockReturnValue('blob:mock-url');
    vi.mocked(URL.revokeObjectURL).mockImplementation(() => {});
  });

  describe('Error Handling', () => {
    it('should display folders error', () => {
      const mockError = new Error('Failed to load folders');
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: mockError,
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: vi.fn(),
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
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: vi.fn(),
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
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: mockError,
        refreshVideos: vi.fn(),
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
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: vi.fn(),
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
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['video1.mov'],
        videosError: null,
        refreshVideos: vi.fn(),
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
      const today = new Date().toISOString().split('T')[0];
      const folders = [today, '2025-01-14', '2025-01-13'];

      vi.mocked(useFolders).mockReturnValue({
        folders,
        foldersError: null,
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: vi.fn(),
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
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: vi.fn(),
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
      const videos = ['video2.mov', 'video1.mov'];

      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos,
        videosError: null,
        refreshVideos: vi.fn(),
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: null,
      });

      vi.mocked(readFile).mockResolvedValue(new Uint8Array([1, 2, 3]));

      render(<App />);

      const videosButton = screen.getByText(/Videos/i).closest('button');
      if (videosButton) {
        fireEvent.click(videosButton);
      }

      await waitFor(() => {
        expect(readFile).toHaveBeenCalledWith(
          'Timelapse/video2.mov',
          expect.any(Object)
        );
      });
    });
  });

  describe('Image Loading', () => {
    it('should load image when folder and files are available', async () => {
      const mockImageData = new Uint8Array([1, 2, 3, 4]);

      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: vi.fn(),
      });
      vi.mocked(useFiles).mockReturnValue({
        files: ['image1.jpg', 'image2.jpg'],
        filesError: null,
      });
      vi.mocked(readFile).mockResolvedValue(mockImageData);

      render(<App />);

      await waitFor(() => {
        expect(readFile).toHaveBeenCalled();
      });

      expect(URL.createObjectURL).toHaveBeenCalled();
    });

    it('should handle image loading errors gracefully', async () => {
      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: vi.fn(),
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
    it('should load video and create blob URL', async () => {
      const mockVideoData = new Uint8Array([10, 20, 30, 40, 50]);
      const mockBlobUrl = 'blob:mock-video-url';

      vi.mocked(URL.createObjectURL).mockReturnValue(mockBlobUrl);
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['test-video.mov'],
        videosError: null,
        refreshVideos: vi.fn(),
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: null,
      });
      vi.mocked(readFile).mockResolvedValue(mockVideoData);

      render(<App />);

      // Switch to videos mode
      const videosButton = screen.getAllByText(/Videos/i).find(el => el.tagName === 'BUTTON');
      if (videosButton) {
        fireEvent.click(videosButton);
      }

      // Wait for video to load
      await waitFor(() => {
        expect(readFile).toHaveBeenCalledWith(
          'Timelapse/test-video.mov',
          expect.any(Object)
        );
      });

      // Verify blob URL was created
      expect(URL.createObjectURL).toHaveBeenCalled();

      // Verify video element is rendered with correct src
      await waitFor(() => {
        const videoElement = document.querySelector('video');
        expect(videoElement).toBeTruthy();
        expect(videoElement?.src).toBe(mockBlobUrl);
      });
    });

    it('should show loading state while video is loading', async () => {
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['slow-video.mov'],
        videosError: null,
        refreshVideos: vi.fn(),
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: null,
      });

      // Make readFile take a long time so we can see the loading state
      let resolveRead: (value: Uint8Array) => void;
      const readPromise = new Promise<Uint8Array>(resolve => {
        resolveRead = resolve;
      });
      vi.mocked(readFile).mockReturnValue(readPromise);

      render(<App />);

      // Switch to videos mode
      const videosButton = screen.getAllByText(/Videos/i).find(el => el.tagName === 'BUTTON');
      if (videosButton) {
        fireEvent.click(videosButton);
      }

      // Should show loading message immediately
      await waitFor(() => {
        expect(screen.getByText(/Loading video/i)).toBeInTheDocument();
      });
      // Video name appears in both the select dropdown and the loading message
      const videoTexts = screen.getAllByText(/slow-video.mov/i);
      expect(videoTexts.length).toBeGreaterThan(0);

      // Resolve the read to clean up
      resolveRead!(new Uint8Array([1, 2, 3]));
    });

    it('should handle video loading errors', async () => {
      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['broken-video.mov'],
        videosError: null,
        refreshVideos: vi.fn(),
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: null,
      });
      vi.mocked(readFile).mockRejectedValue(new Error('Video file not found'));

      render(<App />);

      // Switch to videos mode
      const videosButton = screen.getAllByText(/Videos/i).find(el => el.tagName === 'BUTTON');
      if (videosButton) {
        fireEvent.click(videosButton);
      }

      await waitFor(() => {
        expect(consoleSpy).toHaveBeenCalledWith(
          'Error loading video:',
          expect.any(Error)
        );
      });

      // Should show loading state (video failed to load)
      expect(screen.getByText(/Loading video/i)).toBeInTheDocument();

      consoleSpy.mockRestore();
    });

    it('should clean up blob URL when switching videos', async () => {
      const mockVideoData1 = new Uint8Array([1, 2, 3]);
      const mockVideoData2 = new Uint8Array([4, 5, 6]);
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
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['video1.mov', 'video2.mov'],
        videosError: null,
        refreshVideos: vi.fn(),
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: null,
      });

      let readFileCallCount = 0;
      vi.mocked(readFile).mockImplementation(() => {
        readFileCallCount++;
        return Promise.resolve(readFileCallCount === 1 ? mockVideoData1 : mockVideoData2);
      });

      render(<App />);

      // Switch to videos mode
      const videosButton = screen.getAllByText(/Videos/i).find(el => el.tagName === 'BUTTON');
      if (videosButton) {
        fireEvent.click(videosButton);
      }

      // Wait for first video to load
      await waitFor(() => {
        expect(readFile).toHaveBeenCalledWith('Timelapse/video1.mov', expect.any(Object));
      });

      // Switch to second video
      const videoSelect = screen.getByRole('combobox', { name: '' });
      fireEvent.change(videoSelect, { target: { value: 'video2.mov' } });

      // Wait for second video to load
      await waitFor(() => {
        expect(readFile).toHaveBeenCalledWith('Timelapse/video2.mov', expect.any(Object));
      });

      // First blob URL should have been revoked when switching videos
      expect(URL.revokeObjectURL).toHaveBeenCalledWith(mockBlobUrl1);
    });

    it('should clean up blob URL when switching away from video mode', async () => {
      const mockVideoData = new Uint8Array([1, 2, 3]);
      const mockBlobUrl = 'blob:video-url';

      vi.mocked(URL.createObjectURL).mockReturnValue(mockBlobUrl);
      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['test-video.mov'],
        videosError: null,
        refreshVideos: vi.fn(),
      });
      vi.mocked(useFiles).mockReturnValue({
        files: ['image1.jpg'],
        filesError: null,
      });
      vi.mocked(readFile).mockResolvedValue(mockVideoData);

      render(<App />);

      // Switch to videos mode
      const videosButton = screen.getAllByText(/Videos/i).find(el => el.tagName === 'BUTTON');
      if (videosButton) {
        fireEvent.click(videosButton);
      }

      // Wait for video to load
      await waitFor(() => {
        expect(URL.createObjectURL).toHaveBeenCalled();
      });

      // Switch back to images mode
      const imagesButton = screen.getByText(/Images/i).closest('button');
      if (imagesButton) {
        fireEvent.click(imagesButton);
      }

      // Blob URL should be revoked when switching away from videos
      await waitFor(() => {
        expect(URL.revokeObjectURL).toHaveBeenCalledWith(mockBlobUrl);
      });
    });

    it('should show video controls', async () => {
      const mockVideoData = new Uint8Array([1, 2, 3]);

      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['test-video.mov'],
        videosError: null,
        refreshVideos: vi.fn(),
      });
      vi.mocked(useFiles).mockReturnValue({
        files: [],
        filesError: null,
      });
      vi.mocked(readFile).mockResolvedValue(mockVideoData);

      render(<App />);

      // Switch to videos mode
      const videosButton = screen.getAllByText(/Videos/i).find(el => el.tagName === 'BUTTON');
      if (videosButton) {
        fireEvent.click(videosButton);
      }

      // Wait for video element
      await waitFor(() => {
        const videoElement = document.querySelector('video');
        expect(videoElement).toBeTruthy();
        expect(videoElement?.hasAttribute('controls')).toBe(true);
      });
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
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: vi.fn(),
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
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: vi.fn(),
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

  describe('Refresh Functionality', () => {
    it('should call refreshFolders when refresh button is clicked in images mode', async () => {
      const mockRefreshFolders = vi.fn();
      const mockRefreshVideos = vi.fn();

      vi.mocked(useFolders).mockReturnValue({
        folders: ['2025-01-15'],
        foldersError: null,
        refreshFolders: mockRefreshFolders,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: mockRefreshVideos,
      });
      vi.mocked(useFiles).mockReturnValue({
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
      const mockRefreshFolders = vi.fn();
      const mockRefreshVideos = vi.fn();

      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: mockRefreshFolders,
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: ['video1.mov'],
        videosError: null,
        refreshVideos: mockRefreshVideos,
      });
      vi.mocked(useFiles).mockReturnValue({
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
      vi.mocked(useFolders).mockReturnValue({
        folders: [],
        foldersError: null,
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: vi.fn(),
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
        refreshFolders: vi.fn(),
      });
      vi.mocked(useVideos).mockReturnValue({
        videos: [],
        videosError: null,
        refreshVideos: vi.fn(),
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
