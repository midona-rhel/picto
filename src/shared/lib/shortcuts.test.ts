import { afterEach, describe, expect, it } from 'vitest';
import { getKeyboardPreset, getShortcut, setKeyboardPreset, setShortcutBinding } from './shortcuts';

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
    expect(getShortcut('view.fitWindow')?.keys).toBe('Shift+F');

    setKeyboardPreset('us');
    expect(getShortcut('view.fitWindow')?.keys).toBe('`');
    expect(getShortcut('view.fitWindow')?.keys2).toBe('Shift+F');
  });

  it('resolves persisted overrides through the same registry used by dispatch', () => {
    setShortcutBinding('nav.search', 'keys', 'Mod+Shift+F');
    expect(getShortcut('nav.search')?.keys).toBe('Mod+Shift+F');
    expect(JSON.parse(localStorage.getItem('picto-shortcut-overrides') ?? '{}')).toMatchObject({
      'nav.search': { keys: 'Mod+Shift+F' },
    });
    setShortcutBinding('nav.search', 'keys', 'Mod+F');
  });
});
