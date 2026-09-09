import { describe, expect, it } from 'vitest';
import {
  captureGridScrollPosition,
  collectThumbnailActivation,
  estimateGridScrollHeight,
  GRID_SCROLL_ESTIMATE_SAMPLE_LIMIT,
  GridLayoutRuntime,
  prepareGridLayout,
  restoreGridScrollTop,
  type ThumbnailActivationBuffers,
} from './gridLayoutModel';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { ThumbnailRevealTracker } from './thumbnailRevealTracker';

function item(itemId: number, fileHash = `file-${itemId}`, width = 100, height = 100): CanonicalEntityGridItem {
  return {
    root_id: itemId,
    kind: 'media',
    lifecycle: 'active',
    name: `item-${itemId}`,
    cover_media_id: itemId,
    content_hash: fileHash,
    mime: 'image/jpeg',
    width,
    height,
    duration_ms: null,
    frame_count: null,
    palette: [],
    imported_at_ms: itemId,
    captured_at_ms: null,
    modified_at_ms: itemId,
    rating: 'unrated',
    media_count: 1,
    total_size_bytes: 100,
  };
}

describe('buildGridLayoutModel', () => {
  it('prepares identical visible geometry and rejects warm geometry after a width change', () => {
    const source = Array.from({ length: 1500 }, (_, i) => item(i + 1, `file-${i}`, 100 + i % 300, 200));
    for (const viewMode of ['grid', 'waterfall', 'justified'] as const) {
      const config = { width: 900, targetSize: 200, gap: 16, viewMode, textHeight: 20, scrollbarWidth: 8 };
      const expected = new GridLayoutRuntime().update(source, config);
      prepareGridLayout(source, config);
      const prepared = new GridLayoutRuntime().update(source, config);
      expect(prepared.positions).toEqual(expected.positions);
      expect(prepared.items).toEqual(expected.items);
      prepareGridLayout(source, config);
      const resized = new GridLayoutRuntime().update(source, { ...config, width: 500 });
      expect(resized.positions).toEqual(new GridLayoutRuntime().update(source, { ...config, width: 500 }).positions);
      expect(resized.positions).not.toEqual(expected.positions);
    }
  });
  it('builds render, position, spatial, and item lookup data atomically', () => {
    const model = new GridLayoutRuntime().update([item(10), item(11, 'file-11', 200, 100)], {
      width: 500, targetSize: 180, gap: 16, viewMode: 'grid', textHeight: 20, scrollbarWidth: 8,
    });
    expect(model.items.map((entry) => entry.itemId)).toEqual([10, 11]);
    expect(model.positions).toHaveLength(2);
    expect(model.itemIdToIndex).toEqual(new Map([[10, 0], [11, 1]]));
    expect(model.totalHeight).toBeGreaterThan(0);
    expect(model.scrollEstimateSampleCount).toBe(2);
    expect(model.scrollEstimateSampleHeight).toBe(model.totalHeight);
    expect(model.spatialIndex.queryYRange(0, model.totalHeight, [])).toEqual([0, 1]);
  });

  it('reuses adapted prefix objects only for a true page append', () => {
    const runtime = new GridLayoutRuntime();
    const config = { width: 500, targetSize: 180, gap: 16, viewMode: 'grid' as const, textHeight: 20, scrollbarWidth: 8 };
    const first = [item(10), item(11)];
    const initial = runtime.update(first, config);
    const appended = runtime.update([...first, item(12)], config);
    expect(appended.items[0]).toBe(initial.items[0]);
    expect(appended.itemIdToIndex.get(12)).toBe(2);

    const reordered = runtime.update([first[1], first[0], item(12)], config);
    expect(reordered.items[0]).not.toBe(appended.items[1]);
    expect(reordered.itemIdToIndex).toEqual(new Map([[11, 0], [10, 1], [12, 2]]));
  });
});

describe('estimateGridScrollHeight', () => {
  it('preserves the full result scroll range from loaded layout density', () => {
    expect(estimateGridScrollHeight(2_032, 500, 10_000)).toBe(40_032);
  });

  it('never shrinks below real loaded content', () => {
    expect(estimateGridScrollHeight(2_000, 500, 400)).toBe(2_000);
    expect(estimateGridScrollHeight(2_000, 500, null)).toBe(2_000);
  });

  it('bounds projection input to the first 500 real items', () => {
    const runtime = new GridLayoutRuntime();
    const source = Array.from({ length: GRID_SCROLL_ESTIMATE_SAMPLE_LIMIT + 1 }, (_, index) =>
      item(index + 1, `file-${index + 1}`, index === GRID_SCROLL_ESTIMATE_SAMPLE_LIMIT ? 1 : 100, 100),
    );
    const model = runtime.update(source, {
      width: 500, targetSize: 180, gap: 16, viewMode: 'waterfall', textHeight: 20, scrollbarWidth: 8,
    });
    const firstPage = new GridLayoutRuntime().update(source.slice(0, GRID_SCROLL_ESTIMATE_SAMPLE_LIMIT), {
      width: 500, targetSize: 180, gap: 16, viewMode: 'waterfall', textHeight: 20, scrollbarWidth: 8,
    });

    expect(model.scrollEstimateSampleCount).toBe(GRID_SCROLL_ESTIMATE_SAMPLE_LIMIT);
    expect(model.scrollEstimateSampleHeight).toBe(firstPage.totalHeight);
    expect(estimateGridScrollHeight(
      model.totalHeight,
      source.length,
      10_000,
      model.scrollEstimateSampleHeight,
      model.scrollEstimateSampleCount,
    )).toBe(estimateGridScrollHeight(
      firstPage.totalHeight,
      GRID_SCROLL_ESTIMATE_SAMPLE_LIMIT,
      10_000,
    ));
  });
});

describe('grid scroll restoration', () => {
  it('restores the same relative location when the result height changes', () => {
    const saved = captureGridScrollPosition(4_500, 10_000, 1_000);
    expect(saved).toEqual({ scrollTop: 4_500, progress: 0.5 });
    expect(restoreGridScrollTop(saved, 20_000, 1_000)).toBe(9_500);
  });

  it('preserves the bottom edge across result mutations', () => {
    const saved = captureGridScrollPosition(9_000, 10_000, 1_000);
    expect(saved.progress).toBe(1);
    expect(restoreGridScrollTop(saved, 7_000, 1_000)).toBe(6_000);
  });

  it('falls back to the exact offset before a scroll range can be estimated', () => {
    expect(restoreGridScrollTop({ scrollTop: 320, progress: 0.4 }, 0, 0)).toBe(320);
  });
});

describe('collectThumbnailActivation', () => {
  it('starts reveal eligibility at exactly 500px on either side without changing HQ visibility', () => {
    const model = new GridLayoutRuntime().update(Array.from({ length: 5 }, (_, i) => item(i + 1)), {
      width: 500, targetSize: 180, gap: 16, viewMode: 'grid', textHeight: 0, scrollbarWidth: 8,
    });
    const positions = [399, 400, 1500, 2500, 2501].map(y => ({ x: 0, y, w: 100, h: 100 }));
    const buffers: ThumbnailActivationBuffers = {
      activeTiles: [], activeHashes: new Set(), viewportHashes: new Set(), planTiles: [],
    };
    collectThumbnailActivation([0, 1, 2, 3, 4], positions, model.items,
      300, 2700, 1000, 2000, 0, 'grid', false, buffers);
    expect([...buffers.viewportHashes]).toEqual(['2', '3', '4']);
    expect(buffers.planTiles.map(tile => tile.inViewport)).toEqual([false, false, true, false, false]);
  });

  it('keeps the same reveal lifetime when a nearby tile moves onto the actual screen', () => {
    const model = new GridLayoutRuntime().update([item(1)], {
      width: 500, targetSize: 180, gap: 16, viewMode: 'grid', textHeight: 0, scrollbarWidth: 8,
    });
    const positions = [{ x: 0, y: 2500, w: 100, h: 100 }];
    const buffers: ThumbnailActivationBuffers = {
      activeTiles: [], activeHashes: new Set(), viewportHashes: new Set(), planTiles: [],
    };
    const tracker = new ThumbnailRevealTracker();
    const move = (top: number, now: number) => {
      collectThumbnailActivation([0], positions, model.items,
        top - 600, top + 1600, top, top + 1000, 0, 'grid', false, buffers);
      tracker.updateViewport(buffers.viewportHashes, () => true, now);
    };
    try {
      move(999, 0); // 501px away: not yet eligible.
      expect(buffers.viewportHashes.size).toBe(0);
      move(1000, 10); // Exactly 500px: lifetime begins.
      expect(tracker.getProgress('1', 109)).toBe(0);
      move(1600, 260); // Now visible, but the clock must not restart.
      expect(tracker.getProgress('1', 260)).toBe(0.3);
      expect(tracker.getProgress('1', 610)).toBe(1);
      move(0, 700); // Leave the entire reveal region.
      move(1000, 800);
      expect(tracker.getProgress('1', 899)).toBe(0);
      expect(tracker.getProgress('1', 1150)).toBe(0.5);
    } finally {
      tracker.clear();
    }
  });

  it('keeps fonts visible without scheduling a raster thumbnail decode', () => {
    const source = [item(10), { ...item(11), mime: 'font/ttf' }];
    const model = new GridLayoutRuntime().update(source, {
      width: 500, targetSize: 180, gap: 16, viewMode: 'grid', textHeight: 20, scrollbarWidth: 8,
    });
    const buffers: ThumbnailActivationBuffers = {
      activeTiles: [],
      activeHashes: new Set(),
      viewportHashes: new Set(),
      planTiles: [],
    };

    collectThumbnailActivation(
      [0, 1], model.positions, model.items,
      0, model.totalHeight, 0, model.totalHeight,
      20, 'grid', false, buffers,
    );

    expect(buffers.activeTiles).toEqual([0, 1]);
    expect(buffers.planTiles.map((tile) => tile.fileHash)).toEqual(['file-10']);
    expect(buffers.planTiles[0].h).toBe(model.positions[0].h - 20);
  });
});
