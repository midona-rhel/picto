import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { suppressScrollMomentum } from './scrollMomentum';

function wheel(deltaY: number): WheelEvent {
  const event = new WheelEvent('wheel', { deltaY, cancelable: true, bubbles: true });
  document.body.dispatchEvent(event);
  return event;
}

describe('suppressScrollMomentum', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.runAllTimers(); vi.useRealTimers(); });

  it('swallows a coasting scroll until it pauses', () => {
    suppressScrollMomentum();
    for (const delta of [40, 32, 25, 18, 12]) {
      expect(wheel(delta).defaultPrevented).toBe(true);
      vi.advanceTimersByTime(16);
    }
    vi.advanceTimersByTime(100);
    expect(wheel(30).defaultPrevented).toBe(false);
  });

  it('lets a new, stronger flick through at once', () => {
    suppressScrollMomentum();
    expect(wheel(10).defaultPrevented).toBe(true);
    vi.advanceTimersByTime(16);
    expect(wheel(60).defaultPrevented).toBe(false);
    expect(wheel(55).defaultPrevented).toBe(false);
  });

  it('does nothing when no scroll is under way', () => {
    suppressScrollMomentum();
    vi.advanceTimersByTime(100);
    expect(wheel(5).defaultPrevented).toBe(false);
  });

  it('never swallows input for long', () => {
    suppressScrollMomentum();
    for (let elapsed = 0; elapsed < 1_300; elapsed += 16) {
      wheel(8);
      vi.advanceTimersByTime(16);
    }
    expect(wheel(8).defaultPrevented).toBe(false);
  });
});
