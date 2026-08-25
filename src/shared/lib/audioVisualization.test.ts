import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import {
  getAudioVisualizationMode,
  setAudioVisualizationMode,
  useAudioVisualizationMode,
} from './audioVisualization';

describe('audio visualization preference', () => {
  it('defaults to Spectrum and ignores invalid stored values', () => {
    localStorage.removeItem('picto:audio-visualization');
    expect(getAudioVisualizationMode()).toBe('spectrum');
    localStorage.setItem('picto:audio-visualization', 'unknown');
    expect(getAudioVisualizationMode()).toBe('spectrum');
    localStorage.setItem('picto:audio-visualization', 'bass_reactor');
    expect(getAudioVisualizationMode()).toBe('spectrum');
  });

  it('updates open renderers immediately', () => {
    const { result } = renderHook(() => useAudioVisualizationMode());

    act(() => setAudioVisualizationMode('orbit'));

    expect(result.current).toBe('orbit');
  });
});
