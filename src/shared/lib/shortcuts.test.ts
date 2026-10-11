import { afterEach, describe, expect, it } from 'vitest';
import {
  getKeyboardPreset,
  findShortcutConflict,
  getApplicationMenuShortcutBindings,
  getShortcut,
  gridRemovalAction,
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

describe('folder removal shortcuts', () => {
  const mod = /Mac|iPhone|iPad/.test(navigator.platform) ? { metaKey: true } : { ctrlKey: true };
  const key = (shiftKey = false) => new KeyboardEvent('keydown', { key: 'Backspace', ...mod, shiftKey });

  afterEach(() => {
    localStorage.setItem('picto-shortcut-overrides', '{}');
    reloadShortcutStateFromStorage();
  });

  it('removes only folder membership in folder navigation, not library lifecycle', () => {
    expect(findShortcutConflict('file.removeFromFolder', 'Mod+Backspace')).toBeUndefined();
    expect(findShortcutConflict('file.delete', 'Mod+Backspace')).toBeUndefined();
    expect(gridRemovalAction(key(), true, false)).toBe('remove-from-folder');
    expect(gridRemovalAction(key(true), true, false)).toBeNull();
    expect(gridRemovalAction(new KeyboardEvent('keydown', { key: 'Backspace' }), true, false)).toBeNull();
  });

  it('keeps trash, permanent deletion and restore behavior outside folders', () => {
    expect(gridRemovalAction(key(), false, false)).toBe('trash');
    expect(gridRemovalAction(key(), false, true)).toBe('delete-permanently');
    expect(gridRemovalAction(key(true), false, true)).toBe('restore');
    expect(gridRemovalAction(key(true), false, false)).toBeNull();
  });

  it('honors independent remapping without falling through to trash in folders', () => {
    setShortcutBinding('file.removeFromFolder', 'keys', 'Alt+R');
    expect(gridRemovalAction(key(), true, false)).toBeNull();
    expect(gridRemovalAction(new KeyboardEvent('keydown', { key: 'r', altKey: true }), true, false)).toBe('remove-from-folder');
    expect(gridRemovalAction(key(), false, false)).toBe('trash');
    expect(getApplicationMenuShortcutBindings()).toMatchObject({
      'file.removeFromFolder': 'Alt+R',
      'file.delete': 'Mod+Backspace',
      'file.restore': 'Mod+Shift+Backspace',
    });
  });

  it('allows the folder removal binding to be disabled without enabling trash', () => {
    setShortcutBinding('file.removeFromFolder', 'keys', '');
    expect(gridRemovalAction(key(), true, false)).toBeNull();
    expect(getApplicationMenuShortcutBindings()['file.removeFromFolder']).toBe('');
  });
});
