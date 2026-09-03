import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { CONTROLS_HIDE_DELAY } from './videoConstants';
import { useMediaControlsVisibility } from './useMediaControlsVisibility';

afterEach(() => {
  vi.useRealTimers();
});

describe('useMediaControlsVisibility', () => {
  it('hides after inactivity, reveals on activity, and stays visible during a held interaction', () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(
      ({ holdOpen }) => useMediaControlsVisibility(holdOpen),
      { initialProps: { holdOpen: false } },
    );

    expect(result.current.controlsVisible).toBe(true);
    act(() => vi.advanceTimersByTime(CONTROLS_HIDE_DELAY));
    expect(result.current.controlsVisible).toBe(false);

    act(() => result.current.revealControls());
    expect(result.current.controlsVisible).toBe(true);

    rerender({ holdOpen: true });
    act(() => vi.advanceTimersByTime(CONTROLS_HIDE_DELAY * 2));
    expect(result.current.controlsVisible).toBe(true);

    rerender({ holdOpen: false });
    act(() => vi.advanceTimersByTime(CONTROLS_HIDE_DELAY));
    expect(result.current.controlsVisible).toBe(false);
  });
});
