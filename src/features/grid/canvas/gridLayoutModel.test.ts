import { describe, expect, it } from 'vitest';
import { GridLayoutRuntime } from './gridLayoutModel';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';

function item(itemId: number, fileHash = `file-${itemId}`, width = 100, height = 100): CanonicalEntityGridItem {
  return {
    item_id: itemId,
    kind: 'media',
    lifecycle: 'active',
    label: null,
    name: `item-${itemId}`,
    display_media_item_id: itemId,
    display_file_hash: fileHash,
    display_mime_type: 'image/jpeg',
    pixel_width: width,
    pixel_height: height,
    duration_ms: null,
    frame_count: null,
    has_audio: false,
    dominant_color_hex: null,
    size_bytes: 1,
    rating: null,
    captured_at: null,
    imported_at: null,
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
