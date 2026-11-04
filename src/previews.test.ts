import { describe, expect, it } from 'bun:test';

import { nearest, previewPackUrl, readPack } from './previews';

/** A pack as Rust writes it: `frames` are (frame index, image bytes). */
function pack(frames: Array<[number, Array<number>]>): Blob {
  const head = new DataView(new ArrayBuffer(12 + frames.length * 8));
  'TLPV'.split('').forEach((c, i) => head.setUint8(i, c.charCodeAt(0)));
  head.setUint32(4, 1, true);
  head.setUint32(8, frames.length, true);
  frames.forEach(([index, bytes], i) => {
    head.setUint32(12 + i * 8, index, true);
    head.setUint32(16 + i * 8, bytes.length, true);
  });
  return new Blob([head.buffer, ...frames.map(([, bytes]) => new Uint8Array(bytes))]);
}

describe('readPack', () => {
  it('reads where each image is', async () => {
    const read = await readPack('v.full.pack', pack([[0, [1, 2, 3]], [15, []], [30, [4]]]));
    expect(read?.indices).toEqual([0, 15, 30]);
    expect(read?.starts).toEqual([36, 39, 39]);
    expect(read?.ends).toEqual([39, 39, 40]);
    const image = read!.blob.slice(read!.starts[0], read!.ends[0]);
    expect([...new Uint8Array(await image.arrayBuffer())]).toEqual([1, 2, 3]);
  });

  it('rejects anything else', async () => {
    expect(await readPack('x', new Blob(['not a pack at all']))).toBeNull();
    // Claims more image bytes than it has.
    const truncated = (await pack([[0, [1, 2, 3]]]).arrayBuffer()).slice(0, 22);
    expect(await readPack('x', new Blob([truncated]))).toBeNull();
  });
});

describe('nearest', () => {
  it('finds the closest frame, the earlier one on a tie', () => {
    const indices = [0, 15, 30, 250];
    expect(nearest(indices, 0)).toBe(0);
    expect(nearest(indices, 7)).toBe(0);
    expect(nearest(indices, 8)).toBe(1);
    expect(nearest(indices, 140)).toBe(2);
    expect(nearest(indices, 141)).toBe(3);
    expect(nearest(indices, 9999)).toBe(3);
    expect(nearest([42], 0)).toBe(0);
  });
});

describe('previewPackUrl', () => {
  it('is served by the frames protocol', () => {
    expect(previewPackUrl('v-1.full.pack')).toBe('frames://localhost/previews/v-1.full.pack');
  });
});
