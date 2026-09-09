import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ThumbnailRevealTracker } from './thumbnailRevealTracker';

const bitmapSet = (...hashes: string[]) => {
  const available = new Set(hashes);
  return (hash: string) => available.has(hash);
};

describe('consistent thumbnail reveal', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

  it.each(['cached', 'prefetched', 'newly decoded'])('%s images wait 100ms then reveal over 500ms', (kind) => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(['a']), bitmapSet(...(kind === 'newly decoded' ? [] : ['a'])), 0);
    if (kind === 'newly decoded') tracker.onBitmapAvailable('a', 50);
    expect(tracker.getProgress('a', 99)).toBe(0);
    expect(tracker.getProgress('a', 100)).toBe(0);
    expect(tracker.getProgress('a', 350)).toBe(0.5);
    expect(tracker.getProgress('a', 599)).toBeLessThan(1);
    expect(tracker.getProgress('a', 600)).toBe(1);
  });

  it('gives a late bitmap its full fade without restarting the visibility delay', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(['a']), bitmapSet(), 0);
    tracker.onBitmapAvailable('a', 300);
    expect(tracker.getProgress('a', 300)).toBe(0);
    expect(tracker.getProgress('a', 550)).toBe(0.5);
    expect(tracker.getProgress('a', 800)).toBe(1);
  });

  it('does not replay for reflow, reorder, or a quality upgrade while continuously visible', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(['a', 'b']), bitmapSet('a', 'b'), 0);
    tracker.updateViewport(new Set(['b', 'a']), bitmapSet('a', 'b'), 700);
    tracker.onBitmapAvailable('a', 700);
    expect(tracker.getProgress('a', 700)).toBe(1);
    expect(tracker.getProgress('b', 700)).toBe(1);
  });

  it('repeats the same timing after a cached item leaves and re-enters', () => {
    const tracker = new ThumbnailRevealTracker();
    tracker.updateViewport(new Set(['a']), bitmapSet('a'), 0);
    tracker.updateViewport(new Set(), bitmapSet('a'), 600);
    tracker.updateViewport(new Set(['a']), bitmapSet('a'), 700);
    expect(tracker.getProgress('a', 799)).toBe(0);
    expect(tracker.getProgress('a', 1050)).toBe(0.5);
    expect(tracker.getProgress('a', 1300)).toBe(1);
  });

  it('does not earn reveal time offscreen or spin animation frames during the delay', () => {
    const wake = vi.fn();
    const tracker = new ThumbnailRevealTracker(wake);
    tracker.onBitmapAvailable('a', 0);
    expect(vi.getTimerCount()).toBe(0);
    tracker.updateViewport(new Set(['a', 'b']), bitmapSet('a', 'b'), 200);
    expect(vi.getTimerCount()).toBe(1);
    expect(tracker.getProgress('a', 299)).toBe(0);
    expect(tracker.isAnimating('a', 299)).toBe(false);
    vi.advanceTimersByTime(99);
    expect(wake).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(wake).toHaveBeenCalledOnce();
    expect(tracker.isAnimating('a', 300)).toBe(true);
    expect(tracker.isAnimating('a', 800)).toBe(false);
  });

  it('cancels pending reveals when tiles leave or the grid is disposed', () => {
    const wake = vi.fn();
    const tracker = new ThumbnailRevealTracker(wake);
    tracker.updateViewport(new Set(['a']), bitmapSet('a'), 0);
    tracker.updateViewport(new Set(), bitmapSet(), 50);
    vi.runAllTimers();
    expect(wake).not.toHaveBeenCalled();
    tracker.updateViewport(new Set(['a']), bitmapSet('a'), 200);
    tracker.clear();
    vi.runAllTimers();
    expect(wake).not.toHaveBeenCalled();
  });
});
