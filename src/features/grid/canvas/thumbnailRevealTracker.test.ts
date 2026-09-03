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
    expect(tracker.getProgress('a', 600)).toBe(1);

    tracker.updateViewport(new Set(['b', 'a']), 700, bitmapSet('a', 'b'));
    expect(tracker.getProgress('a', 700)).toBe(1);
    expect(tracker.getProgress('b', 700)).toBe(1);
  });

  it('reveals once again after an entity fully leaves and re-enters', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(['a']), 0, bitmapSet('a'));
    expect(tracker.getProgress('a', 600)).toBe(1);

    tracker.updateViewport(new Set(), 600, bitmapSet('a'));
    tracker.updateViewport(new Set(['a']), 700, bitmapSet('a'));
    expect(tracker.getProgress('a', 700)).toBe(0);
    expect(tracker.getProgress('a', 799)).toBe(0);
    expect(tracker.getProgress('a', 1050)).toBe(0.5);
  });

  it('holds a cached bitmap for the full viewport dwell before revealing it', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(), 0, bitmapSet('a'));
    tracker.updateViewport(new Set(['a']), 100, bitmapSet('a'));
    expect(tracker.getProgress('a', 100)).toBe(0);
    expect(tracker.getProgress('a', 199)).toBe(0);
    expect(tracker.getProgress('a', 200)).toBe(0);
    expect(tracker.getProgress('a', 450)).toBe(0.5);
  });

  it('starts a visible entity reveal when its bitmap arrives', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(['a']), 0, bitmapSet());
    tracker.onBitmapAvailable('a', 50);
    expect(tracker.getProgress('a', 99)).toBe(0);
    expect(tracker.getProgress('a', 350)).toBe(0.5);
  });

  it('does not request animation frames while a cached bitmap is still dwelling', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(['a']), 0, bitmapSet('a'));

    expect(tracker.isAnimating('a', 99)).toBe(false);
    expect(tracker.isAnimating('a', 100)).toBe(true);
    expect(tracker.isAnimating('a', 600)).toBe(false);
  });

  it('does not start a reveal when an offscreen prefetch completes', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(), 0, bitmapSet());
    tracker.onBitmapAvailable('a', 100);
    tracker.updateViewport(new Set(['a']), 200, bitmapSet('a'));
    expect(tracker.getProgress('a', 200)).toBe(0);
    expect(tracker.getProgress('a', 299)).toBe(0);
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
