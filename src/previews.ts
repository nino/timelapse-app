import { invoke } from "@tauri-apps/api/core";

import { framesBase } from "./frames";

/** One video's preview pack, placed in its day. Mirrors `DayPreviewPack` in Rust. */
export type PreviewPack = {
  /** File name, served at `frames://localhost/previews/<name>`. */
  name: string;
  /** Day-wide index of the video's first frame. */
  firstIndex: number;
  frameCount: number;
  /** Only the video's keyframes, until its full pack is built. */
  coarse: boolean;
};

/** Mirrors `DayPreviews` in Rust. */
export type DayPreviews = {
  frameCount: number;
  packs: Array<PreviewPack>;
};

/** The preview packs of `date`'s videos. Videos without one yet are left out. */
export function getDayPreviews(date: string): Promise<DayPreviews> {
  return invoke<DayPreviews>("get_day_previews", { date });
}

/** URL of the preview pack called `name`, from the same `frames` protocol as frames. */
export function previewPackUrl(name: string): string {
  return `${framesBase()}/previews/${name}`;
}

/**
 * A pack's images, read from its header (see `frame_source::previews` in
 * Rust): little-endian u32s `"TLPV"`, version 1, a count, then per image its
 * frame index in the video and its length, then the JPEGs back to back. The
 * bytes stay in the `Blob`, outside the JS heap, and each image is handed out
 * as an object URL of a slice of it, made the first time it is asked for.
 */
export type LoadedPack = {
  name: string;
  /** Frame index within the video of each image, ascending. */
  indices: Array<number>;
  /** Byte range of each image within `blob`. */
  starts: Array<number>;
  ends: Array<number>;
  blob: Blob;
  urls: Map<number, string>;
};

/** Parse a pack, or null if it isn't one. */
export async function readPack(name: string, blob: Blob): Promise<LoadedPack | null> {
  if (blob.size < 12) return null;
  const head = new DataView(await blob.slice(0, 12).arrayBuffer());
  const magic = String.fromCharCode(
    head.getUint8(0),
    head.getUint8(1),
    head.getUint8(2),
    head.getUint8(3),
  );
  if (magic !== "TLPV" || head.getUint32(4, true) !== 1) return null;
  const count = head.getUint32(8, true);
  const tableEnd = 12 + count * 8;
  if (blob.size < tableEnd) return null;
  const table = new DataView(await blob.slice(12, tableEnd).arrayBuffer());
  const indices: Array<number> = [];
  const starts: Array<number> = [];
  const ends: Array<number> = [];
  let at = tableEnd;
  for (let i = 0; i < count; i++) {
    const length = table.getUint32(i * 8 + 4, true);
    indices.push(table.getUint32(i * 8, true));
    starts.push(at);
    ends.push(at + length);
    at += length;
  }
  if (at > blob.size) return null;
  return { name, indices, starts, ends, blob, urls: new Map() };
}

/** Position in `indices` (ascending) of the value nearest `target`. */
export function nearest(indices: Array<number>, target: number): number {
  let lo = 0;
  let hi = indices.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (indices[mid] < target) lo = mid + 1;
    else hi = mid;
  }
  // `lo` is the first at or after `target`; the one before may be nearer.
  if (lo > 0 && target - indices[lo - 1] <= indices[lo] - target) return lo - 1;
  return lo;
}

/** Object URL of image `i` of `pack`, made once and kept until `releasePack`. */
export function imageUrl(pack: LoadedPack, i: number): string {
  let url = pack.urls.get(i);
  if (url === undefined) {
    url = URL.createObjectURL(pack.blob.slice(pack.starts[i], pack.ends[i], "image/jpeg"));
    pack.urls.set(i, url);
  }
  return url;
}

/** Revoke the object URLs handed out for `pack`. */
export function releasePack(pack: LoadedPack): void {
  for (const url of pack.urls.values()) URL.revokeObjectURL(url);
  pack.urls.clear();
}
