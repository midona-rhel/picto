import { afterEach, describe, expect, it } from 'vitest';
import { getKeyboardPreset, getShortcut, setKeyboardPreset } from './shortcuts';

describe('keyboard presets', () => {
  afterEach(() => {
    setKeyboardPreset('us');
  });

  it('restores US bindings after switching back from EU', () => {
    const fitWindow = getShortcut('view.fitWindow');
    expect(fitWindow).toBeDefined();
    expect(getKeyboardPreset()).toBe('us');
    expect(fitWindow?.keys).toBe('`');

    setKeyboardPreset('eu');
    expect(fitWindow?.keys).toBe('Shift+F');

    setKeyboardPreset('us');
    expect(fitWindow?.keys).toBe('`');
    expect(fitWindow?.keys2).toBe('Shift+F');
  });
});
