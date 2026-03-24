import { describe, expect, it } from 'vitest';

import {
  classifyCanvasScrollPhase,
  resolveCanvasScrollDirection,
} from '../scrollState';

describe('canvas scroll state', () => {
  it('classifies fast scrolling by velocity', () => {
    expect(classifyCanvasScrollPhase(2200)).toBe('fast');
    expect(classifyCanvasScrollPhase(600)).toBe('slow');
    expect(classifyCanvasScrollPhase(0)).toBe('idle');
  });

  it('resolves scroll direction from delta sign', () => {
    expect(resolveCanvasScrollDirection(12)).toBe('forward');
    expect(resolveCanvasScrollDirection(-12)).toBe('backward');
    expect(resolveCanvasScrollDirection(0)).toBe('unknown');
  });
});
