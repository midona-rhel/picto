import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { suppressScrollMomentum } from './scrollMomentum';

function wheel(deltaY: number): WheelEvent {
  const event = new WheelEvent('wheel', { deltaY, cancelable: true, bubbles: true });
  document.body.dispatchEvent(event);
  return event;
}

describe('suppressScrollMomentum', () => {
  // Each test starts well after the last one's wheel input.
  let now = Date.now();
  beforeEach(() => {
    vi.useFakeTimers();
    now += 60_000;
    vi.setSystemTime(now);
  });
  afterEach(() => { vi.runAllTimers(); vi.useRealTimers(); });

  /** A scroll already coasting when the view switches. */
  function coasting(deltaY: number) {
    vi.advanceTimersByTime(1_000);
    wheel(deltaY);
    vi.advanceTimersByTime(16);
  }

  it('swallows a coasting scroll until it pauses', () => {
    coasting(48);
    suppressScrollMomentum();
    for (const delta of [40, 32, 25, 18, 12]) {
      expect(wheel(delta).defaultPrevented).toBe(true);
      vi.advanceTimersByTime(16);
    }
    vi.advanceTimersByTime(100);
    expect(wheel(30).defaultPrevented).toBe(false);
  });

  it('lets a new, stronger flick through at once', () => {
    coasting(12);
    suppressScrollMomentum();
    expect(wheel(10).defaultPrevented).toBe(true);
    vi.advanceTimersByTime(16);
    expect(wheel(60).defaultPrevented).toBe(false);
    expect(wheel(55).defaultPrevented).toBe(false);
  });

  it('does nothing when no scroll is under way', () => {
    vi.advanceTimersByTime(1_000);
    suppressScrollMomentum();
    // Not even the first notch of a scroll started right after the switch.
    expect(wheel(5).defaultPrevented).toBe(false);
    vi.advanceTimersByTime(16);
    expect(wheel(5).defaultPrevented).toBe(false);
  });

  it('never swallows input for long', () => {
    coasting(8);
    suppressScrollMomentum();
    for (let elapsed = 0; elapsed < 1_300; elapsed += 16) {
      wheel(8);
      vi.advanceTimersByTime(16);
    }
    expect(wheel(8).defaultPrevented).toBe(false);
  });
});
