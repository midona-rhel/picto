import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ThumbnailDecodeQuality } from './thumbnailDecodeClient';

let deliverBitmap: ((hash: string, bitmap: ImageBitmap, quality: ThumbnailDecodeQuality) => void) | null = null;
let deliverError: ((hash: string, quality: ThumbnailDecodeQuality) => void) | null = null;

vi.mock('./thumbnailDecodeClient', () => ({
  ThumbnailDecodeClient: class {
    constructor(onBitmap: typeof deliverBitmap, onError: typeof deliverError) {
      deliverBitmap = onBitmap;
      deliverError = onError;
    }
    sendPlan() {}
    invalidate() {}
    clear() {}
    terminate() {}
  },
}));

import { ThumbnailPipeline } from './thumbnailPipeline';

function bitmap(width = 100, height = 100): ImageBitmap {
  return { width, height, close: vi.fn() } as unknown as ImageBitmap;
}

describe('ThumbnailPipeline full-resolution admission', () => {
  beforeEach(() => {
    deliverBitmap = null;
    deliverError = null;
  });

  it('installs at most one full-resolution bitmap per animation frame', () => {
    const frames: FrameRequestCallback[] = [];
    const onDirty = vi.fn();
    const pipeline = new ThumbnailPipeline(
      onDirty,
      vi.fn(),
      (callback) => { frames.push(callback); return frames.length; },
      vi.fn(),
    );
    const first = bitmap(2000, 1500);
    const second = bitmap(1800, 2400);

    deliverBitmap?.('first', first, 'full');
    deliverBitmap?.('second', second, 'full');

    expect(pipeline.get('first')).toBeNull();
    expect(pipeline.get('second')).toBeNull();
    expect(frames).toHaveLength(1);

    frames.shift()?.(0);
    expect(pipeline.get('first')?.thumb).toBe(first);
    expect(pipeline.get('second')).toBeNull();
    expect(onDirty).toHaveBeenCalledTimes(1);
    expect(frames).toHaveLength(1);

    frames.shift()?.(16);
    expect(pipeline.get('second')?.thumb).toBe(second);
    expect(onDirty).toHaveBeenCalledTimes(2);
  });

  it('keeps a usable thumbnail when a full-resolution replacement fails', () => {
    const pipeline = new ThumbnailPipeline(vi.fn(), vi.fn(), vi.fn(), vi.fn());
    const thumbnail = bitmap(512, 512);

    deliverBitmap?.('item', thumbnail, 'thumbnail');
    expect(pipeline.get('item')?.thumb).toBe(thumbnail);

    deliverError?.('item', 'full');
    expect(pipeline.get('item')?.thumb).toBe(thumbnail);
    expect(pipeline.get('item')?.state).toBe('shown');
  });

  it('closes queued full-resolution bitmaps during teardown', () => {
    const frames: FrameRequestCallback[] = [];
    const cancelFrame = vi.fn();
    const pipeline = new ThumbnailPipeline(
      vi.fn(),
      vi.fn(),
      (callback) => { frames.push(callback); return 7; },
      cancelFrame,
    );
    const queued = bitmap(3000, 2000);

    deliverBitmap?.('queued', queued, 'full');
    pipeline.destroy();

    expect(queued.close).toHaveBeenCalledOnce();
    expect(cancelFrame).toHaveBeenCalledWith(7);
  });
});
