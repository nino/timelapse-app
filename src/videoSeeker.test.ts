import { describe, expect, it } from "bun:test";

import { MOVING_MS, SETTLE_MS, VideoSeeker, type SeekableVideo } from "./videoSeeker";

const FPS = 15;
// Keyframes every 250 frames, like the converter's x265 encodes.
const GOP = 250;

/** A paused video that finishes each seek when the test says so. */
class FakeVideo implements SeekableVideo {
  private time = 0;
  readyState = 1;
  seeking = false;
  isConnected = true;
  seeks: Array<{ kind: "exact" | "fast"; frame: number }> = [];
  private listeners = new Map<string, Array<() => void>>();
  frameCallbacks: Array<(now: number, metadata: { mediaTime: number }) => void> = [];

  private readonly withFrameCallback: boolean;

  constructor(withFrameCallback = true) {
    this.withFrameCallback = withFrameCallback;
  }

  fastSeek = (time: number): void => {
    const keyframe = Math.floor(Math.floor(time * FPS) / GOP) * GOP;
    this.seeks.push({ kind: "fast", frame: Math.floor(time * FPS) });
    this.time = keyframe / FPS;
    this.seeking = true;
  };

  get currentTime(): number {
    return this.time;
  }

  /** An exact seek. */
  set currentTime(time: number) {
    this.seeks.push({ kind: "exact", frame: Math.floor(time * FPS) });
    this.time = time;
    this.seeking = true;
  }

  get requestVideoFrameCallback():
    | ((callback: (now: number, metadata: { mediaTime: number }) => void) => number)
    | undefined {
    if (!this.withFrameCallback) return undefined;
    return (callback): number => this.frameCallbacks.push(callback);
  }

  addEventListener(type: string, listener: () => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  /** The running seek finishes: `seeked`, then the new frame is presented. */
  land(): void {
    this.seeking = false;
    this.emit("seeked");
    const callbacks = this.frameCallbacks;
    this.frameCallbacks = [];
    const mediaTime = Math.floor(this.time * FPS) / FPS;
    for (const callback of callbacks) callback(0, { mediaTime });
  }

  emit(type: string): void {
    for (const listener of this.listeners.get(type) ?? []) listener();
  }
}

function setup(withFrameCallback = true): {
  video: FakeVideo;
  seeker: VideoSeeker;
  shown: Array<[number, number]>;
  clock: { now: number };
} {
  const video = new FakeVideo(withFrameCallback);
  const shown: Array<[number, number]> = [];
  const clock = { now: 0 };
  const seeker = new VideoSeeker(
    video,
    FPS,
    { onShown: (wanted, frame): void => void shown.push([wanted, frame]) },
    (): number => clock.now,
  );
  return { video, seeker, shown, clock };
}

describe("VideoSeeker", () => {
  it("seeks exactly to the middle of a frame and reports what was shown", () => {
    const { video, seeker, shown } = setup();
    seeker.want(100);
    expect(video.seeks).toEqual([{ kind: "exact", frame: 100 }]);
    expect(video.currentTime).toBeCloseTo(100.5 / FPS);
    video.land();
    expect(shown).toEqual([[100, 100]]);
  });

  it("runs one seek at a time and then goes to the latest frame asked for", () => {
    const { video, seeker, shown } = setup();
    seeker.want(10);
    seeker.want(11);
    seeker.want(12);
    expect(video.seeks).toHaveLength(1);
    video.land();
    expect(shown).toEqual([[10, 10]]);
    expect(video.seeks.map((s) => s.frame)).toEqual([10, 12]);
    video.land();
    expect(shown).toEqual([[10, 10], [12, 12]]);
  });

  it("jumps to keyframes while dragging far, then lands exactly once the drag stops", async () => {
    const { video, seeker, shown, clock } = setup();
    seeker.want(0);
    video.land();
    clock.now += 16;
    seeker.want(900);
    expect(video.seeks.at(-1)).toEqual({ kind: "fast", frame: 900 });
    video.land();
    // Shown the keyframe before frame 900.
    expect(shown.at(-1)).toEqual([900, 750]);
    // Nothing else asked for: the exact seek follows after a pause.
    clock.now += SETTLE_MS;
    await new Promise((resolve) => setTimeout(resolve, SETTLE_MS + 20));
    expect(video.seeks.at(-1)).toEqual({ kind: "exact", frame: 900 });
    video.land();
    expect(shown.at(-1)).toEqual([900, 900]);
  });

  it("steps exactly when the frame asked for is near, even in quick succession", () => {
    const { video, seeker, clock } = setup();
    seeker.want(500);
    video.land();
    for (let frame = 501; frame < 505; frame++) {
      clock.now += MOVING_MS / 3;
      seeker.want(frame);
      video.land();
    }
    expect(video.seeks.every((s) => s.kind === "exact")).toBe(true);
  });

  it("waits for the video's metadata before seeking", () => {
    const { video, seeker, shown } = setup();
    video.readyState = 0;
    seeker.want(42);
    expect(video.seeks).toHaveLength(0);
    video.readyState = 1;
    video.emit("loadedmetadata");
    expect(video.seeks).toEqual([{ kind: "exact", frame: 42 }]);
    video.land();
    expect(shown).toEqual([[42, 42]]);
  });

  it("ignores a frame presented while the seek is still running", () => {
    const { video, seeker, shown } = setup();
    seeker.want(300);
    // The page shows the video's first frame before the seek gets anywhere.
    video.seeking = true;
    for (const callback of video.frameCallbacks.splice(0)) callback(0, { mediaTime: 0 });
    expect(shown).toEqual([]);
    video.land();
    expect(shown).toEqual([[300, 300]]);
  });

  it("uses seeked and currentTime where there is no frame callback", () => {
    const { video, seeker, shown } = setup(false);
    seeker.want(77);
    video.land();
    expect(shown).toEqual([[77, 77]]);
  });

  it("reports at once when the frame asked for is already shown", () => {
    const { video, seeker, shown } = setup();
    seeker.want(5);
    video.land();
    seeker.want(6);
    video.land();
    seeker.want(5);
    video.land();
    expect(shown).toEqual([[5, 5], [6, 6], [5, 5]]);
    expect(video.seeks).toHaveLength(3);
  });
});
