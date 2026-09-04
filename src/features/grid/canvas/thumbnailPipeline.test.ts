import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ThumbnailDecodePlanEntry, ThumbnailDecodeQuality } from './thumbnailDecodeClient';

let deliverBitmap: ((hash: string, bitmap: ImageBitmap, quality: ThumbnailDecodeQuality) => void) | null = null;
let deliverError: ((hash: string, quality: ThumbnailDecodeQuality) => void) | null = null;
const invalidated: string[] = [];
const plans: ThumbnailDecodePlanEntry[][] = [];

vi.mock('./thumbnailDecodeClient', () => ({
  ThumbnailDecodeClient: class {
    constructor(onBitmap: typeof deliverBitmap, onError: typeof deliverError) {
      deliverBitmap = onBitmap;
      deliverError = onError;
    }
    sendPlan(entries: ThumbnailDecodePlanEntry[]) {
      plans.push(entries.map((entry) => ({ ...entry })));
    }
    invalidate(hash: string) { invalidated.push(hash); }
    clear() {}
    terminate() {}
  },
}));

import {
  FULL_QUALITY_VIEWPORT_DWELL_MS,
  THUMBNAIL_VIEWPORT_DWELL_MS,
  ThumbnailPipeline,
} from './thumbnailPipeline';

function bitmap(width = 100, height = 100): ImageBitmap {
  return { width, height, close: vi.fn() } as unknown as ImageBitmap;
}

function activate(pipeline: ThumbnailPipeline, ...hashes: string[]): void {
  pipeline.updatePlan(hashes.map(fileHash => ({
    fileHash, mime: 'image/png', w: 200, h: 200, cy: 100, inViewport: true,
  })), 100, 1, 0);
}

describe('ThumbnailPipeline viewport admission', () => {
  beforeEach(() => {
    deliverBitmap = null;
    deliverError = null;
    plans.length = 0;
  });

  it('waits 100ms for a thumbnail and 250ms total for a display-sized original', () => {
    const scheduledDelays: number[] = [];
    const pipeline = new ThumbnailPipeline(
      vi.fn(),
      vi.fn(),
      vi.fn(),
      vi.fn(),
      (_callback, delayMs) => { scheduledDelays.push(delayMs); return scheduledDelays.length; },
      vi.fn(),
    );
    const cannon = {
      fileHash: 'cannon',
      mime: 'image/png',
      w: 474,
      h: 723,
      sourceWidth: 2755,
      sourceHeight: 4200,
      fit: 'cover',
      inViewport: true,
      cy: 400,
    } as const;

    pipeline.updatePlan([cannon], 400, 1, 0);
    expect(plans).toEqual([[]]);

    pipeline.updatePlan([cannon], 400, 1, THUMBNAIL_VIEWPORT_DWELL_MS - 1);
    expect(plans).toEqual([[]]);

    pipeline.updatePlan([cannon], 400, 1, THUMBNAIL_VIEWPORT_DWELL_MS);
    expect(plans[plans.length - 1]).toEqual([{
      fileHash: 'cannon',
      url: 'media://localhost/thumb/cannon.jpg?v=0',
      quality: 'thumbnail',
      resizeWidth: undefined,
      resizeHeight: undefined,
    }]);

    deliverBitmap?.('cannon', bitmap(335, 512), 'thumbnail');
    pipeline.updatePlan([cannon], 400, 1, FULL_QUALITY_VIEWPORT_DWELL_MS - 1);
    expect(plans[plans.length - 1]).toEqual([]);

    pipeline.updatePlan([cannon], 400, 1, FULL_QUALITY_VIEWPORT_DWELL_MS);

    expect(plans[plans.length - 1]).toEqual([{
      fileHash: 'cannon',
      url: 'media://localhost/file/cannon.png',
      quality: 'full',
      resizeWidth: 475,
      resizeHeight: 723,
    }]);
    expect(scheduledDelays).toContain(THUMBNAIL_VIEWPORT_DWELL_MS);
  });

  it('resets dwell time when a tile leaves the viewport during a fast scroll', () => {
    const pipeline = new ThumbnailPipeline(vi.fn(), vi.fn(), vi.fn(), vi.fn(), vi.fn(), vi.fn());
    const tile = {
      fileHash: 'passing-item',
      mime: 'image/jpeg',
      w: 200,
      h: 200,
      cy: 100,
      inViewport: true,
    };

    pipeline.updatePlan([tile], 100, 1, 0);
    tile.inViewport = false;
    pipeline.updatePlan([tile], 100, 1, 50);
    tile.inViewport = true;
    pipeline.updatePlan([tile], 100, 1, 60);
    pipeline.updatePlan([tile], 100, 1, 159);
    expect(plans).toEqual([[]]);

    pipeline.updatePlan([tile], 100, 1, 160);
    expect(plans[plans.length - 1]?.[0]).toMatchObject({
      fileHash: 'passing-item',
      quality: 'thumbnail',
    });
  });

  it('wakes the grid when the next visible tile reaches its dwell threshold', () => {
    const scheduledCallbacks: Array<() => void> = [];
    const onDirty = vi.fn();
    const pipeline = new ThumbnailPipeline(
      onDirty,
      vi.fn(),
      vi.fn(),
      vi.fn(),
      (callback) => { scheduledCallbacks.push(callback); return 1; },
      vi.fn(),
    );

    pipeline.updatePlan([{
      fileHash: 'waiting-item',
      mime: 'image/jpeg',
      w: 200,
      h: 200,
      cy: 100,
      inViewport: true,
    }], 100, 1, 0);
    scheduledCallbacks[0]();

    expect(onDirty).toHaveBeenCalledOnce();
  });
});

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
    activate(pipeline, 'first', 'second');

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
    pipeline.destroy();
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
    activate(pipeline, 'receiver');

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
    activate(pipeline, 'item');

    deliverBitmap?.('item', thumbnail, 'thumbnail');
    expect(pipeline.get('item')?.thumb).toBe(thumbnail);

    deliverError?.('item', 'full');
    expect(pipeline.get('item')?.thumb).toBe(thumbnail);
    expect(pipeline.get('item')?.state).toBe('shown');
    pipeline.destroy();
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
    activate(pipeline, 'queued');

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
    pipeline.destroy();
  });

  it('removes evicted metadata and closes late bitmaps from old windows', () => {
    const pipeline = new ThumbnailPipeline(vi.fn(), vi.fn(), vi.fn(), vi.fn(), vi.fn(), vi.fn());
    activate(pipeline, 'old');
    const old = bitmap();
    deliverBitmap?.('old', old, 'thumbnail');
    activate(pipeline, 'new');
    pipeline.evictOutsideActive(new Set(['new']));
    expect(old.close).toHaveBeenCalledOnce();
    expect(pipeline.get('old')).toBeNull();
    const late = bitmap();
    deliverBitmap?.('old', late, 'thumbnail');
    expect(late.close).toHaveBeenCalledOnce();
    expect(pipeline.get('old')).toBeNull();
    pipeline.destroy();
  });
});
