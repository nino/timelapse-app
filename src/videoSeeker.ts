/**
 * Moves a paused `<video>` to the frame the viewer wants, for the beta video
 * player (`VideoFrame.tsx`).
 *
 * Only one seek runs at a time: a frame asked for while one is running
 * waits, and only the latest such frame is sought once it has finished, so
 * dragging the scrubber never queues seeks. Every seek is exact. Measured in
 * WKWebView on an M4 Max (2026-11-04), an exact seek takes ~33 ms and a drag
 * shows a new frame about every display refresh, while `fastSeek` was no
 * faster and lands on the keyframe before the target, up to 250 frames off.
 *
 * `requestVideoFrameCallback` says which frame was presented; where the
 * browser has none, the `seeked` event and `currentTime` stand in for it.
 */

/** The parts of a video element the seeker uses, so tests can fake one. */
export type SeekableVideo = {
  currentTime: number;
  readonly readyState: number;
  readonly seeking: boolean;
  isConnected: boolean;
  requestVideoFrameCallback?: (
    callback: (now: number, metadata: { mediaTime: number }) => void,
  ) => number;
  addEventListener: (type: string, listener: () => void) => void;
};

export type SeekerCallbacks = {
  /** The seek asked for frame `wanted` has finished, showing frame `shown`. */
  onShown: (wanted: number, shown: number) => void;
};

/**
 * How long to wait after `seeked` for the frame callback. A seek to the frame
 * already shown presents nothing new, so the callback may never come.
 */
const FRAME_CALLBACK_GRACE_MS = 100;
// `HTMLMediaElement.HAVE_METADATA`.
const HAVE_METADATA = 1;

export class VideoSeeker {
  private wanted: number | null = null;
  private shown: number | null = null;
  /** The frame the running seek is for, and its number. */
  private running: { frame: number; id: number } | null = null;
  private seeks = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;

  private readonly video: SeekableVideo;
  private readonly fps: number;
  callbacks: SeekerCallbacks;

  constructor(video: SeekableVideo, fps: number, callbacks: SeekerCallbacks) {
    this.video = video;
    this.fps = fps;
    this.callbacks = callbacks;
    video.addEventListener("loadedmetadata", () => this.start());
    video.addEventListener("seeked", () => this.onSeeked());
  }

  /** Show frame `frame` of the video (0-based). */
  want(frame: number): void {
    if (frame === this.wanted && (this.running !== null || frame === this.shown)) return;
    this.wanted = frame;
    this.start();
  }

  private start(): void {
    const frame = this.wanted;
    // A running seek keeps its timer: after `seeked` it is what finishes the
    // seek when no frame callback comes.
    if (frame === null || this.running !== null || this.video.readyState < HAVE_METADATA) return;
    this.clearTimer();
    if (frame === this.shown) {
      this.callbacks.onShown(frame, frame);
      return;
    }
    const id = ++this.seeks;
    this.running = { frame, id };
    this.video.requestVideoFrameCallback?.((_, metadata) => this.onFrame(id, metadata.mediaTime));
    // The middle of the frame, so rounding can't land on its neighbour.
    this.video.currentTime = (frame + 0.5) / this.fps;
  }

  private onFrame(id: number, mediaTime: number): void {
    if (this.running?.id !== id) return;
    // A frame from before the seek (the first one a new video shows).
    if (this.video.seeking) {
      this.video.requestVideoFrameCallback?.((_, metadata) => this.onFrame(id, metadata.mediaTime));
      return;
    }
    this.finish(this.frameAt(mediaTime));
  }

  private onSeeked(): void {
    const running = this.running;
    if (running === null) return;
    if (this.video.requestVideoFrameCallback === undefined) {
      this.finish(this.frameAt(this.video.currentTime));
      return;
    }
    this.clearTimer();
    this.timer = setTimeout(() => {
      if (this.running?.id === running.id) this.finish(this.frameAt(this.video.currentTime));
    }, FRAME_CALLBACK_GRACE_MS);
  }

  private finish(shown: number): void {
    const running = this.running;
    if (running === null) return;
    this.running = null;
    this.shown = shown;
    this.clearTimer();
    if (!this.video.isConnected) return;
    this.callbacks.onShown(running.frame, shown);
    // Asked for another frame meanwhile: go there.
    if (this.wanted !== running.frame) this.start();
  }

  private frameAt(time: number): number {
    // Frame n starts at n / fps; the slack absorbs rounding in the media time.
    return Math.max(0, Math.floor(time * this.fps + 0.01));
  }

  private clearTimer(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }
}
