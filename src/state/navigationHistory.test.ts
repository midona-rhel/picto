import { getDefaultStore } from 'jotai';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { activeNodeIdAtom } from './navigation';
import { gridController } from '../controllers/gridController';
import {
  goBack,
  goForward,
  getScrollPosition,
  navigateToNode,
  navigateWithGridFilters,
  resetNavigationHistory,
  saveScrollPosition,
} from './navigationHistory';
import { gridDrilldownAtom, gridFilterLockedAtom, gridSessionAtom, pendingGridIntentAtom, pendingGridNavigationAtom } from './grid';
import { createEmptyItemFilters } from '../shared/lib/itemFilters';
import {
  quickLookSessionAtom,
  viewerDisplayControlsAtom,
  viewerDisplayStateAtom,
  viewerExitTransitionAtom,
  viewerSessionAtom,
} from './viewer';

const store = getDefaultStore();

beforeEach(() => resetNavigationHistory());

afterEach(() => {
  store.set(viewerSessionAtom, null);
  store.set(quickLookSessionAtom, null);
  store.set(viewerDisplayStateAtom, null);
  store.set(viewerDisplayControlsAtom, null);
  store.set(viewerExitTransitionAtom, false);
  store.set(activeNodeIdAtom, 'system:active');
  store.set(gridFilterLockedAtom, false);
});

describe('navigateToNode', () => {
  it('records the actual toolbar command so Back restores a numeric filter after leaving its scope', () => {
    store.set(activeNodeIdAtom, 'system:active');
    const filters = { ...createEmptyItemFilters(), min_width: 1024n };
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), scope: { kind: 'all' }, filters: createEmptyItemFilters() });
    gridController.setFilters(filters);
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), filters });
    navigateToNode('folder:7');
    goBack();
    expect(store.get(pendingGridNavigationAtom)).toMatchObject({
      nodeId: 'system:active', filters, restoreScroll: true,
    });
    goBack();
    expect(store.get(pendingGridIntentAtom)).toMatchObject({ filters: createEmptyItemFilters() });
    goForward();
    expect(store.get(pendingGridIntentAtom)).toMatchObject({ filters });
  });

  it('keeps the manager owner when the toolbar filters a drill-down', () => {
    store.set(activeNodeIdAtom, 'system:tag_manager');
    const filters = { ...createEmptyItemFilters(), min_width: 1024n };
    store.set(gridDrilldownAtom, {
      ownerNodeId: 'system:tag_manager', scopeNodeId: 'system:active', filters: createEmptyItemFilters(),
    });
    gridController.setFilters(filters);
    expect(store.get(activeNodeIdAtom)).toBe('system:tag_manager');
    expect(store.get(gridDrilldownAtom)).toMatchObject({ ownerNodeId: 'system:tag_manager', filters });
    expect(store.get(pendingGridNavigationAtom)).toMatchObject({ nodeId: 'system:active', filters });
  });

  it('keeps the outgoing viewer alive for the scope transition', () => {
    const session = { currentIndex: 2, currentItemId: 42 };
    store.set(viewerSessionAtom, session);
    store.set(quickLookSessionAtom, session);
    store.set(viewerDisplayStateAtom, { currentIndex: 2, total: 10 });
    store.set(viewerDisplayControlsAtom, { close: () => undefined });

    navigateToNode('system:trash');

    expect(store.get(activeNodeIdAtom)).toBe('system:trash');
    expect(store.get(viewerSessionAtom)).toEqual(session);
    expect(store.get(quickLookSessionAtom)).toEqual(session);
    expect(store.get(viewerExitTransitionAtom)).toBe(true);
  });

  it('closes the viewer when the active scope is chosen again', () => {
    store.set(activeNodeIdAtom, 'folder:7');
    store.set(viewerSessionAtom, { currentIndex: 0, currentItemId: 7 });

    navigateToNode('folder:7');

    expect(store.get(activeNodeIdAtom)).toBe('folder:7');
    expect(store.get(viewerSessionAtom)).toBeNull();
  });

  it('keeps the viewer mounted while history starts a scope transition', () => {
    navigateToNode('system:inbox');
    navigateToNode('system:trash');
    store.set(viewerSessionAtom, { currentIndex: 0, currentItemId: 9 });

    goBack();

    expect(store.get(activeNodeIdAtom)).toBe('system:inbox');
    expect(store.get(viewerSessionAtom)).not.toBeNull();
    expect(store.get(viewerExitTransitionAtom)).toBe(true);
  });

  it('restores the previous filters when history stays in the same scope', () => {
    navigateToNode('system:inbox');
    const before = createEmptyItemFilters();
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), filters: before });
    navigateWithGridFilters('system:inbox', { ...before, include_tags: [{ tag_id: 1, name: 'artist:alice' }] });
    store.set(pendingGridIntentAtom, null);

    goBack();

    expect(store.get(activeNodeIdAtom)).toBe('system:inbox');
    expect(store.get(pendingGridIntentAtom)).toEqual({
      type: 'filter',
      filters: before,
      restoreScroll: true,
    });
  });

  it('records numeric filters in history and restores them through Back and Forward', () => {
    const before = createEmptyItemFilters();
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), filters: before });
    // The grid applies each filter intent before the next navigation.
    const visit = (filters: typeof before) => {
      navigateWithGridFilters('system:active', filters);
      store.set(gridSessionAtom, { ...store.get(gridSessionAtom), filters: { ...filters } });
    };
    const wide = { ...before, min_width: 100n, max_size_bytes: 5_000_000n };
    visit(wide);
    // An equal value is the same visit; a different bound is a new one.
    visit({ ...before, min_width: 100n, max_size_bytes: 5_000_000n });
    visit({ ...wide, min_width: 200n });
    wide.min_width = 999n; // the editor's object changing later does not rewrite history
    store.set(pendingGridIntentAtom, null);

    goBack();
    expect(store.get(pendingGridIntentAtom)).toEqual({
      type: 'filter',
      filters: { ...before, min_width: 100n, max_size_bytes: 5_000_000n },
      restoreScroll: true,
    });
    goBack();
    expect(store.get(pendingGridIntentAtom)).toMatchObject({ filters: before });
    goForward();
    goForward();
    expect(store.get(pendingGridIntentAtom)).toMatchObject({
      filters: { ...before, min_width: 200n, max_size_bytes: 5_000_000n },
    });
  });

  it('keeps independent scroll positions for filtered visits in the same scope', () => {
    const allPosition = { scrollTop: 7_900_000, progress: 0.99 };
    const filteredPosition = { scrollTop: 360, progress: 0.2 };
    const filters = {
      ...createEmptyItemFilters(),
      include_tags: [{ tag_id: 1, name: 'artist:alice' }],
    };

    saveScrollPosition('system:active', allPosition);
    navigateWithGridFilters('system:active', filters);
    saveScrollPosition('system:active', filteredPosition);

    goBack();
    expect(getScrollPosition('system:active')).toEqual(allPosition);

    goForward();
    expect(getScrollPosition('system:active')).toEqual(filteredPosition);
  });

  it('marks direct filtered navigation as a fresh top-level visit', () => {
    const filters = { ...createEmptyItemFilters(), include_tags: [{ tag_id: 1, name: 'artist:alice' }] };

    navigateWithGridFilters('system:active', filters);

    expect(store.get(pendingGridIntentAtom)).toEqual({
      type: 'filter',
      filters,
      restoreScroll: false,
    });
  });

  it('marks Back navigation to another scope for scroll restoration', () => {
    navigateToNode('system:inbox');
    saveScrollPosition('system:inbox', { scrollTop: 400, progress: 0.5 });
    navigateToNode('system:trash');

    goBack();

    expect(store.get(pendingGridNavigationAtom)?.restoreScroll).toBe(true);
  });

  it('carries locked filters into the next grid scope and its history entry', () => {
    const filters = { ...createEmptyItemFilters(), include_tags: [{ tag_id: 1, name: 'favorite' }] };
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), filters });
    store.set(gridFilterLockedAtom, true);

    navigateToNode('system:inbox');

    expect(store.get(pendingGridNavigationAtom)).toMatchObject({ nodeId: 'system:inbox', filters });
    store.set(pendingGridNavigationAtom, null);
    navigateToNode('system:trash');
    goBack();
    expect(store.get(pendingGridNavigationAtom)?.filters).toEqual(filters);
  });

  it('keeps a manager selected while its filtered grid is open and Back restores the manager', () => {
    navigateToNode('system:tag_manager');
    const filters = { ...createEmptyItemFilters(), include_tags: [{ tag_id: 1, name: 'artist:alice' }] };

    navigateWithGridFilters('system:active', filters, 'system:tag_manager');

    expect(store.get(activeNodeIdAtom)).toBe('system:tag_manager');
    expect(store.get(gridDrilldownAtom)).toEqual({
      ownerNodeId: 'system:tag_manager',
      scopeNodeId: 'system:active',
      filters,
    });

    goBack();
    expect(store.get(activeNodeIdAtom)).toBe('system:tag_manager');
    expect(store.get(gridDrilldownAtom)).toBeNull();
  });

  it('restores the same random order when returning through history', () => {
    navigateToNode('system:random');
    const firstVisit = store.get(pendingGridNavigationAtom);
    expect(firstVisit?.sort).toMatchObject({ field: 'random', direction: 'ascending' });
    expect(firstVisit?.sort?.random_seed).toBeTruthy();

    store.set(pendingGridNavigationAtom, null);
    navigateToNode('system:inbox');
    goBack();

    expect(store.get(activeNodeIdAtom)).toBe('system:random');
    expect(store.get(pendingGridNavigationAtom)?.sort).toEqual(firstVisit?.sort);
  });
});
