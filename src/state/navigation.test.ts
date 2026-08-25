import { createStore } from 'jotai';
import { beforeEach, describe, expect, it } from 'vitest';
import {
  inspectorCollapsedAtom,
  sidebarCollapsedAtom,
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
});
