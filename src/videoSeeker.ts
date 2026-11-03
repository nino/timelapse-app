/**
 * Moves a paused `<video>` to the frame the viewer wants, for the beta video
 * player (`VideoFrame.tsx`).
 *
 * Only one seek runs at a time: a frame asked for while one is running
 * waits, and only the latest such frame is sought once it has finished, so
 * dragging the scrubber never queues seeks. While the wanted frame keeps
 * changing and is far from the one shown, `fastSeek` jumps to the nearest
 * keyframe, which needs no decoding past it; once the frame has stayed put
 * for `SETTLE_MS`, an exact seek lands on it. Single steps (arrow keys,
 * playback) always seek exactly.
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
  fastSeek?: (time: number) => void;
  requestVideoFrameCallback?: (
    callback: (now: number, metadata: { mediaTime: number }) => void,
  ) => number;
  addEventListener: (type: string, listener: () => void) => void;
};

export type SeekerCallbacks = {
  /** The seek asked for frame `wanted` has finished, showing frame `shown`. */
  onShown: (wanted: number, shown: number) => void;
};

/** A frame asked for within this long of the previous one counts as dragging. */
export const MOVING_MS = 150;
/** How long the wanted frame must stay put before a fast seek is corrected. */
export const SETTLE_MS = 200;
/** Frames this close to the one shown are always sought exactly. */
export const NEAR_FRAMES = 30;
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
  private lastWantAt = -Infinity;
  private timer: ReturnType<typeof setTimeout> | null = null;

  private readonly video: SeekableVideo;
  private readonly fps: number;
  callbacks: SeekerCallbacks;
  private readonly now: () => number;

  constructor(
    video: SeekableVideo,
    fps: number,
    callbacks: SeekerCallbacks,
    now: () => number = (): number => performance.now(),
  ) {
    this.video = video;
    this.fps = fps;
    this.callbacks = callbacks;
    this.now = now;
    video.addEventListener("loadedmetadata", () => this.start());
    video.addEventListener("seeked", () => this.onSeeked());
  }

  /** Show frame `frame` of the video (0-based). */
  want(frame: number): void {
    const now = this.now();
    const moving = now - this.lastWantAt < MOVING_MS;
    this.lastWantAt = now;
    if (frame === this.wanted && (this.running !== null || frame === this.shown)) return;
    this.wanted = frame;
    this.start(moving);
  }

  private start(moving = false): void {
    const frame = this.wanted;
    // A running seek keeps its timer: after `seeked` it is what finishes the
    // seek when no frame callback comes.
    if (frame === null || this.running !== null || this.video.readyState < HAVE_METADATA) return;
    this.clearTimer();
    if (frame === this.shown) {
      this.callbacks.onShown(frame, frame);
      return;
    }
    const far = this.shown === null || Math.abs(frame - this.shown) > NEAR_FRAMES;
    const fast = moving && far && this.video.fastSeek !== undefined;
    const id = ++this.seeks;
    this.running = { frame, id };
    // The middle of the frame, so rounding can't land on its neighbour.
    const time = (frame + 0.5) / this.fps;
    this.video.requestVideoFrameCallback?.((_, metadata) => this.onFrame(id, metadata.mediaTime));
    if (fast) this.video.fastSeek?.(time);
    else this.video.currentTime = time;
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
    if (this.wanted !== running.frame) {
      // Asked for another frame meanwhile: go there, fast if still dragging.
      this.start(this.now() - this.lastWantAt < MOVING_MS);
    } else if (shown !== running.frame) {
      // A fast seek landed on a keyframe: land exactly once the drag stops.
      this.timer = setTimeout(() => this.start(false), SETTLE_MS);
    }
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
