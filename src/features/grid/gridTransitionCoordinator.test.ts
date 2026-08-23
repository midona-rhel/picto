import { afterEach, describe, expect, it, vi } from 'vitest';
import { GridTransitionCoordinator } from './gridTransitionCoordinator';

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('GridTransitionCoordinator', () => {
  it('replaces a pending midpoint instead of running stale work', () => {
    vi.useFakeTimers();
    const coordinator = new GridTransitionCoordinator();
    const first = vi.fn();
    const second = vi.fn();
    coordinator.scheduleDelay(first, 170);
    coordinator.scheduleDelay(second, 170);
    vi.advanceTimersByTime(170);
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledOnce();
  });

  it('cancels both delayed and frame work on teardown', () => {
    vi.useFakeTimers();
    const cancelAnimationFrame = vi.spyOn(window, 'cancelAnimationFrame');
    vi.spyOn(window, 'requestAnimationFrame').mockReturnValue(42);
    const coordinator = new GridTransitionCoordinator();
    const delayed = vi.fn();
    coordinator.scheduleDelay(delayed, 170);
    coordinator.scheduleFrame(vi.fn());
    coordinator.cancel();
    vi.runAllTimers();
    expect(delayed).not.toHaveBeenCalled();
    expect(cancelAnimationFrame).toHaveBeenCalledWith(42);
  });
});
