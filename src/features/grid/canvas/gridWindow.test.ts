import { afterEach, describe, expect, it, vi } from 'vitest';
import { GridLayoutRuntime } from './gridLayoutModel';
import { gridViewportMissesWindow, gridWindowDestination, MAX_GRID_SCROLL_HEIGHT, placeGridWindow, CoalescedGridRequest } from './gridWindow';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';

function layout(count: number) {
  const items = Array.from({ length: count }, (_, i): CanonicalEntityGridItem => ({
    root_id: i + 1, kind: 'media', lifecycle: 'active', name: `Sample ${i}`,
    cover_media_id: i + 1, content_hash: `hash-${i}`, mime: 'image/png',
    width: 200 + i % 3 * 100, height: 300, duration_ms: null, frame_count: 1,
    palette: [], imported_at_ms: i, captured_at_ms: null, modified_at_ms: i,
    rating: 'unrated', media_count: 1, total_size_bytes: 100,
  }));
  return new GridLayoutRuntime().update(items, {
    width: 1200, targetSize: 180, gap: 8, viewMode: 'waterfall', textHeight: 20, scrollbarWidth: 8,
  });
}

describe('continuous-scroll destination loading', () => {
  afterEach(() => { vi.useRealTimers(); });
  it('requests the latest destination within 40 ms even while scrolling continues', () => {
    vi.useFakeTimers();
    const requests = new CoalescedGridRequest();
    const old = vi.fn();
    const latest = vi.fn();
    requests.schedule(old);
    vi.advanceTimersByTime(39);
    expect(old).not.toHaveBeenCalled();
    requests.schedule(latest);
    vi.advanceTimersByTime(1);
    expect(latest).toHaveBeenCalledOnce();
    expect(old).not.toHaveBeenCalled();
    requests.schedule(latest);
    for (let i = 0; i < 4; i++) {
      vi.advanceTimersByTime(10);
      requests.schedule(latest);
    }
    expect(latest).toHaveBeenCalledTimes(2);
    requests.cancel();
  });
  it('does not request an abandoned destination after cancellation', () => {
    vi.useFakeTimers();
    const requests = new CoalescedGridRequest();
    const fetch = vi.fn();
    requests.schedule(fetch);
    requests.cancel();
    vi.runAllTimers();
    expect(fetch).not.toHaveBeenCalled();
  });
});

describe('million-item window geometry', () => {
  it('keeps only loaded positions and exposes the exact first and last tile', () => {
    const local = layout(1500);
    const head = placeGridWindow(local, 0, 1_000_000, 50);
    const tail = placeGridWindow(local, 998500, 1_000_000, 50);
    expect(head.positions).toHaveLength(1500);
    expect(head.totalHeight).toBe(MAX_GRID_SCROLL_HEIGHT);
    expect(head.windowTop).toBe(0);
    expect(tail.windowBottom).toBe(tail.totalHeight);
    expect(gridWindowDestination(head, 0, 1_000_000, head.totalHeight - 800, 800)).toBe(999999);
    expect(gridWindowDestination(tail, 998500, 1_000_000, 0, 800)).toBe(0);
  });
  it('places the guessed destination at the requested position without changing tile sizes', () => {
    const local = layout(1500);
    const model = placeGridWindow(local, 499500, 1_000_000, 50, { index: 500000, top: 4_000_000 });
    expect(model.positions[500].y).toBe(4_000_000);
    expect(model.positions[500].h).toBe(local.positions[500].h);
    expect(gridWindowDestination(model, 499500, 1_000_000, 4_000_000, 800)).toBeNull();
  });
  it('requests neighbors at either edge but nothing when the result is fully loaded', () => {
    const local = layout(1500);
    const model = placeGridWindow(local, 500000, 1_000_000, 50);
    expect(gridWindowDestination(model, 500000, 1_000_000, model.windowTop, 800)).toBeGreaterThanOrEqual(500000);
    expect(gridWindowDestination(model, 500000, 1_000_000, model.windowBottom - 800, 800)).toBeGreaterThan(501000);
    const small = placeGridWindow(local, 0, 1500, 50);
    expect(small.totalHeight).toBe(local.totalHeight);
    expect(gridWindowDestination(small, 0, 1500, 0, 800)).toBeNull();
  });
  it('recognizes a restored viewport that has no resident tiles', () => {
    const model = placeGridWindow(layout(500), 0, 1_000_000, 50);
    expect(gridViewportMissesWindow(model, 0, 800)).toBe(false);
    expect(gridViewportMissesWindow(model, model.totalHeight - 800, 800)).toBe(true);
  });
  it('reveals a fully loaded short result even when the viewport center is below its tiles', () => {
    for (const count of [1, 3]) {
      const model = placeGridWindow(layout(count), 0, count, 50);
      expect(model.totalHeight).toBeLessThan(400);
      expect(gridViewportMissesWindow(model, 0, 800)).toBe(false);
    }
  });
});
