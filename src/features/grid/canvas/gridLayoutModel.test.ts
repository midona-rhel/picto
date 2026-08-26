import { describe, expect, it } from 'vitest';
import {
  captureGridScrollPosition,
  collectThumbnailActivation,
  estimateGridScrollHeight,
  GRID_SCROLL_ESTIMATE_SAMPLE_LIMIT,
  GridLayoutRuntime,
  restoreGridScrollTop,
  type ThumbnailActivationBuffers,
} from './gridLayoutModel';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';

function item(itemId: number, fileHash = `file-${itemId}`, width = 100, height = 100): CanonicalEntityGridItem {
  return {
    item_id: itemId,
    kind: 'media',
    lifecycle: 'active',
    name: `item-${itemId}`,
    display_file_hash: fileHash,
    display_mime_type: 'image/jpeg',
    pixel_width: width,
    pixel_height: height,
    duration_ms: null,
    frame_count: null,
    dominant_color_hex: null,
    rating: null,
    media_count: 1,
  };
}

describe('buildGridLayoutModel', () => {
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
  it('keeps fonts visible without scheduling a raster thumbnail decode', () => {
    const source = [item(10), { ...item(11), display_mime_type: 'font/ttf' }];
    const model = new GridLayoutRuntime().update(source, {
      width: 500, targetSize: 180, gap: 16, viewMode: 'grid', textHeight: 20, scrollbarWidth: 8,
    });
    const buffers: ThumbnailActivationBuffers = {
      activeTiles: [],
      activeHashes: new Set(),
      viewportHashes: new Set(),
      planTiles: [],
    };

    collectThumbnailActivation([0, 1], model.positions, model.items, 0, model.totalHeight, 0, model.totalHeight, buffers);

    expect(buffers.activeTiles).toEqual([0, 1]);
    expect(buffers.planTiles.map((tile) => tile.fileHash)).toEqual(['file-10']);
  });
});
