import { describe, expect, it } from 'vitest';
import { ThumbnailRevealTracker } from './thumbnailRevealTracker';

const bitmapSet = (...hashes: string[]) => {
  const available = new Set(hashes);
  return (hash: string) => available.has(hash);
};

describe('ThumbnailRevealTracker', () => {
  it('does not replay reveals for continuously visible hashes after reorder or reflow', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(['a', 'b']), 0, bitmapSet('a', 'b'));
    expect(tracker.getProgress('a', 500)).toBe(1);

    tracker.updateViewport(new Set(['b', 'a']), 600, bitmapSet('a', 'b'));
    expect(tracker.getProgress('a', 600)).toBe(1);
    expect(tracker.getProgress('b', 600)).toBe(1);
  });

  it('reveals once again after an entity fully leaves and re-enters', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(['a']), 0, bitmapSet('a'));
    expect(tracker.getProgress('a', 500)).toBe(1);

    tracker.updateViewport(new Set(), 600, bitmapSet('a'));
    tracker.updateViewport(new Set(['a']), 700, bitmapSet('a'));
    expect(tracker.getProgress('a', 700)).toBe(0);
    expect(tracker.getProgress('a', 950)).toBe(0.5);
  });

  it('starts a prefetched bitmap reveal when it enters the viewport', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(), 0, bitmapSet('a'));
    tracker.updateViewport(new Set(['a']), 100, bitmapSet('a'));
    expect(tracker.getProgress('a', 100)).toBe(0);
  });

  it('starts a visible entity reveal when its bitmap arrives', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(['a']), 0, bitmapSet());
    expect(tracker.getProgress('a', 100)).toBe(0);
    tracker.onBitmapAvailable('a', 100);
    expect(tracker.getProgress('a', 350)).toBe(0.5);
  });

  it('does not start a reveal when an offscreen prefetch completes', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(), 0, bitmapSet());
    tracker.onBitmapAvailable('a', 100);
    tracker.updateViewport(new Set(['a']), 200, bitmapSet('a'));
    expect(tracker.getProgress('a', 200)).toBe(0);
  });

  it('shows suppressed entries immediately without replaying after suppression', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(['a']), 0, bitmapSet(), true);
    tracker.onBitmapAvailable('a', 100, true);
    expect(tracker.getProgress('a', 100)).toBe(1);
    tracker.updateViewport(new Set(['a']), 200, bitmapSet('a'));
    expect(tracker.getProgress('a', 200)).toBe(1);
  });
});
