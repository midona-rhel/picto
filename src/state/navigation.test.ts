import { createStore } from 'jotai';
import { beforeEach, describe, expect, it } from 'vitest';
import {
  inspectorCollapsedAtom,
  setSidebarWidthAtom,
  sidebarCollapsedAtom,
  sidebarWidthAtom,
  toggleBothPanelsAtom,
} from './navigation';

describe('panel visibility state', () => {
  beforeEach(() => localStorage.clear());

  it('toggles and persists both rails through one action', () => {
    const store = createStore();
    store.set(sidebarCollapsedAtom, false);
    store.set(inspectorCollapsedAtom, true);

    store.set(toggleBothPanelsAtom);

    expect(store.get(sidebarCollapsedAtom)).toBe(true);
    expect(store.get(inspectorCollapsedAtom)).toBe(false);
    expect(localStorage.getItem('picto-sidebar-collapsed-panel')).toBe('true');
    expect(localStorage.getItem('picto-inspector-collapsed')).toBe('false');
  });

  it('clamps and persists the sidebar width', () => {
    const store = createStore();

    store.set(setSidebarWidthAtom, 120);
    expect(store.get(sidebarWidthAtom)).toBe(220);
    expect(localStorage.getItem('picto-sidebar-width')).toBe('220');

    store.set(setSidebarWidthAtom, 900);
    expect(store.get(sidebarWidthAtom)).toBe(500);
    expect(localStorage.getItem('picto-sidebar-width')).toBe('500');
  });
});
