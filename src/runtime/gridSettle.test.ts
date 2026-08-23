import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { gridController } from '../controllers/gridController';
import { gridSessionAtom, gridTransitionPhaseAtom } from '../state/grid';
import { classifyGridAction, processStateChange, startGridSettle } from './gridSettle';

const { stateChangedListeners } = vi.hoisted(() => ({
  stateChangedListeners: [] as Array<(event: { payload: { changes: Record<string, unknown> } }) => void>,
}));

vi.mock('../platform/ipc', () => ({
  listen: vi.fn((_event: string, handler: (event: { payload: { changes: Record<string, unknown> } }) => void) => {
    stateChangedListeners.push(handler);
    return Promise.resolve(() => {});
  }),
}));

const store = getDefaultStore();

describe('grid runtime settling', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), items: [], active: true });
    store.set(gridTransitionPhaseAtom, 'idle');
    stateChangedListeners.length = 0;
  });

  it('settles system lifecycle changes even when extra scopes are present', () => {
    expect(classifyGridAction(
      {
        status_changed: true,
        extra_grid_scopes: ['smart:7'],
      },
      { kind: 'system', key: 'all' },
      [],
    )).toBe('reconcile_membership');
  });

  it('reloads the canonical query after tag structure changes', () => {
    expect(classifyGridAction(
      { tag_structure_changed: true },
      { kind: 'smart_folder', id: 7 },
      [],
    )).toBe('reconcile_membership');
  });

  it('settles an affected smart folder even when extra scopes omit it', () => {
    expect(classifyGridAction(
      {
        smart_folder_ids: [7],
        extra_grid_scopes: ['system:active'],
      },
      { kind: 'smart_folder', id: 7 },
      [],
    )).toBe('reconcile_membership');
  });

  it('ignores unrelated smart-folder changes', () => {
    expect(classifyGridAction(
      { smart_folder_ids: [8] },
      { kind: 'smart_folder', id: 7 },
      [],
    )).toBe('ignore');
  });

  it.each([
    ['Trash', { kind: 'system', key: 'trash' }],
    ['folder', { kind: 'folder', id: 7 }],
    ['smart folder', { kind: 'smart_folder', id: 9 }],
    ['Inbox', { kind: 'system', key: 'inbox' }],
    ['All', { kind: 'system', key: 'all' }],
  ] as const)('reloads the canonical query after an import in %s', (_label, scope) => {
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), scope });
    const reload = vi.spyOn(gridController, 'loadFirstPage').mockResolvedValue(undefined);

    processStateChange({
      entity_hashes: ['new-active-entity'],
      status_changed: true,
    });

    expect(reload).toHaveBeenCalledWith({ preserveItems: true });
  });

  it('coalesces all transition-time events into one canonical reload', async () => {
    const reload = vi.spyOn(gridController, 'loadFirstPage').mockResolvedValue(undefined);
    store.set(gridTransitionPhaseAtom, 'waiting');
    const stop = startGridSettle();
    const listener = stateChangedListeners[stateChangedListeners.length - 1];
    expect(listener).toBeDefined();

    listener!({ payload: { changes: { status_changed: true } } });
    listener!({ payload: { changes: { media_derivatives_changed: true } } });
    expect(reload).not.toHaveBeenCalled();

    store.set(gridTransitionPhaseAtom, 'idle');
    await Promise.resolve();
    expect(reload).toHaveBeenCalledTimes(1);
    expect(reload).toHaveBeenCalledWith({ preserveItems: true });

    stop();
  });
});
