import React from "react";

import { videoUrl, type VideoSpan } from "./frames";
import { VideoSeeker } from "./videoSeeker";

/** One seeker per video element, made the first time the element is seen. */
const seekers = new WeakMap<HTMLVideoElement, VideoSeeker>();

/**
 * The beta video player: shows frame `index` of the day by seeking a paused
 * `<video>` of the video that holds it (`span`), instead of asking the frame
 * source to decode it. See `videoSeeker.ts` for how it seeks.
 *
 * Stays invisible until it has shown a frame, so whatever is underneath (the
 * last `<img>` frame) stays up meanwhile instead of a blank.
 */
export function VideoFrame({
  span,
  index,
  visible,
  onShown,
  onSize,
  onError,
}: {
  span: VideoSpan;
  /** Day-wide index of the frame to show. */
  index: number;
  visible: boolean;
  /** The seek for day-wide frame `wanted` finished, showing frame `shown`. */
  onShown: (wanted: number, shown: number) => void;
  onSize: (size: { width: number; height: number }) => void;
  onError: () => void;
}): React.ReactNode {
  const local = index - span.start;
  const { start, fps } = span;
  // Called again whenever the frame changes, which is what tells the seeker.
  const attach = React.useCallback(
    (video: HTMLVideoElement | null): void => {
      if (!video) return;
      let seeker = seekers.get(video);
      const callbacks = {
        onShown: (wanted: number, shown: number): void => onShown(start + wanted, start + shown),
      };
      if (seeker) {
        seeker.callbacks = callbacks;
      } else {
        seeker = new VideoSeeker(video, fps, callbacks);
        seekers.set(video, seeker);
      }
      seeker.want(local);
    },
    [local, start, fps, onShown],
  );
  return (
    <video
      // A new element per video, so one video's seeker never sees another's.
      key={span.file}
      ref={attach}
      src={videoUrl(span.file)}
      muted
      playsInline
      preload="auto"
      disablePictureInPicture
      aria-label={`Frame ${index + 1}`}
      onLoadedMetadata={(e) => {
        const { videoWidth: width, videoHeight: height } = e.currentTarget;
        if (width > 0 && height > 0) onSize({ width, height });
      }}
      onError={onError}
      className={`w-full h-full object-contain absolute inset-0 ${visible ? "" : "invisible"}`}
    />
  );
}
