import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ThumbnailDecodeQuality } from './thumbnailDecodeClient';

let deliverBitmap: ((hash: string, bitmap: ImageBitmap, quality: ThumbnailDecodeQuality) => void) | null = null;
let deliverError: ((hash: string, quality: ThumbnailDecodeQuality) => void) | null = null;
const invalidated: string[] = [];
const plans: Array<Array<{ fileHash: string; url: string; quality: ThumbnailDecodeQuality }>> = [];

vi.mock('./thumbnailDecodeClient', () => ({
  ThumbnailDecodeClient: class {
    constructor(onBitmap: typeof deliverBitmap, onError: typeof deliverError) {
      deliverBitmap = onBitmap;
      deliverError = onError;
    }
    sendPlan(entries: Array<{ fileHash: string; url: string; quality: ThumbnailDecodeQuality }>) {
      plans.push(entries.map((entry) => ({ ...entry })));
    }
    invalidate(hash: string) { invalidated.push(hash); }
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
    invalidated.length = 0;
    plans.length = 0;
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

  it('preserves the browser receiver when scheduling full-resolution admission', () => {
    const frames: FrameRequestCallback[] = [];
    const schedule = vi.spyOn(window, 'requestAnimationFrame').mockImplementation(function schedule(
      this: Window,
      callback: FrameRequestCallback,
    ) {
      expect(this).toBe(window);
      frames.push(callback);
      return 17;
    });
    const cancel = vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(function cancel(
      this: Window,
      handle: number,
    ) {
      expect(this).toBe(window);
      expect(handle).toBe(17);
    });
    const pipeline = new ThumbnailPipeline();

    expect(() => deliverBitmap?.('receiver', bitmap(1600, 1200), 'full')).not.toThrow();
    expect(frames).toHaveLength(1);
    pipeline.destroy();
    expect(schedule).toHaveBeenCalledOnce();
    expect(cancel).toHaveBeenCalledOnce();

    schedule.mockRestore();
    cancel.mockRestore();
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

  it('loads a thumbnail before upgrading a large tile to the original', () => {
    const pipeline = new ThumbnailPipeline(vi.fn(), vi.fn(), vi.fn(), vi.fn());
    const tile = { fileHash: 'large', mime: 'image/png', w: 1200, h: 900, cy: 100 };

    pipeline.updatePlan([tile], 100);
    expect(plans[plans.length - 1]?.[0]).toMatchObject({
      fileHash: 'large',
      quality: 'thumbnail',
      url: 'media://localhost/thumb/large.jpg?v=0',
    });

    deliverBitmap?.('large', bitmap(512, 384), 'thumbnail');
    pipeline.updatePlan([tile], 100);
    expect(plans[plans.length - 1]?.[0]).toMatchObject({
      fileHash: 'large',
      quality: 'full',
      url: 'media://localhost/file/large.png',
    });

    deliverError?.('large', 'full');
    expect(pipeline.get('large')?.thumb).not.toBeNull();
    expect(pipeline.get('large')?.state).toBe('shown');
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

  it('ignores completion events for thumbnails outside the active decode zone', () => {
    const onDirty = vi.fn();
    const pipeline = new ThumbnailPipeline(onDirty, vi.fn(), vi.fn(), vi.fn());
    pipeline.updatePlan([
      { fileHash: 'visible', mime: 'image/png', w: 200, h: 200, cy: 100 },
    ], 100);

    pipeline.invalidate('offscreen');
    expect(invalidated).toEqual([]);
    expect(onDirty).not.toHaveBeenCalled();

    pipeline.invalidate('visible');
    expect(invalidated).toEqual(['visible']);
    expect(onDirty).toHaveBeenCalledOnce();
  });
});
