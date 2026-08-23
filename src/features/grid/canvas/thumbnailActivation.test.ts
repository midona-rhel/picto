import { describe, expect, it } from 'vitest';
import type { CanvasRenderItem } from './renderItemAdapter';
import { collectThumbnailActivation } from './thumbnailActivation';

const item = (hash: string): CanvasRenderItem => ({
  hash,
  thumbnailHash: hash,
  name: null,
  mime: 'image/jpeg',
  width: 100,
  height: 100,
  rating: null,
  durationMs: null,
  dominantColor: null,
  aspectRatio: 1,
  numFrames: null,
});

function buffers() {
  return {
    activeTiles: [] as number[],
    visibleTiles: [] as number[],
    activeHashes: new Set<string>(),
    viewportHashes: new Set<string>(),
    planTiles: [],
  };
}

describe('collectThumbnailActivation', () => {
  it('separates the preload activation margin from the actual viewport', () => {
    const output = buffers();
    collectThumbnailActivation(
      [0, 1, 2],
      [
        { x: 0, y: -80, w: 100, h: 50 },
        { x: 0, y: 20, w: 100, h: 50 },
        { x: 0, y: 160, w: 100, h: 50 },
      ],
      [item('prefetch-above'), item('visible'), item('prefetch-below')],
      -100,
      200,
      0,
      100,
      output,
    );

    expect(output.activeHashes).toEqual(new Set(['prefetch-above', 'visible', 'prefetch-below']));
    expect(output.visibleTiles).toEqual([1]);
    expect(output.viewportHashes).toEqual(new Set(['visible']));
  });

  it('tracks viewport identity by hash when layout order changes', () => {
    const output = buffers();
    collectThumbnailActivation(
      [0, 1],
      [{ x: 0, y: 0, w: 100, h: 100 }, { x: 110, y: 0, w: 100, h: 100 }],
      [item('b'), item('a')],
      -100,
      200,
      0,
      100,
      output,
    );
    expect(output.viewportHashes).toEqual(new Set(['b', 'a']));
  });
});
