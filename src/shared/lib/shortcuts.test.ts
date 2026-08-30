import { afterEach, describe, expect, it } from 'vitest';
import {
  getKeyboardPreset,
  getShortcut,
  matchesShortcutDef,
  reloadShortcutStateFromStorage,
  setKeyboardPreset,
  setShortcutBinding,
} from './shortcuts';

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

  it('reloads bindings persisted by another window', () => {
    localStorage.setItem('picto-keyboard-preset', 'eu');
    localStorage.setItem('picto-shortcut-overrides', JSON.stringify({
      'nav.search': { keys: 'Mod+Shift+F' },
    }));

    reloadShortcutStateFromStorage();

    expect(getKeyboardPreset()).toBe('eu');
    expect(getShortcut('view.fitWindow')?.keys).toBe('Shift+F');
    expect(getShortcut('nav.search')?.keys).toBe('Mod+Shift+F');

    localStorage.setItem('picto-shortcut-overrides', '{}');
    reloadShortcutStateFromStorage();
  });

  it('allows Shift to extend a remapped grid movement shortcut', () => {
    setShortcutBinding('grid.moveRight', 'keys', 'E');
    const shortcut = getShortcut('grid.moveRight');
    expect(shortcut).toBeDefined();
    expect(matchesShortcutDef(new KeyboardEvent('keydown', { key: 'e', shiftKey: true }), shortcut!, {
      allowExtraShift: true,
    })).toBe(true);
    expect(matchesShortcutDef(new KeyboardEvent('keydown', { key: 'e', shiftKey: true }), shortcut!)).toBe(false);
    setShortcutBinding('grid.moveRight', 'keys', 'ArrowRight');
  });

  it('uses the inbox decision keys for duplicate sides and exposes keep both', () => {
    expect(getShortcut('dup.keepLeft')?.keys).toBe('Z');
    expect(getShortcut('dup.keepRight')?.keys).toBe('X');
    expect(getShortcut('dup.keepBoth')?.keys).toBe('B');
  });
});
