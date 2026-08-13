import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';

let eventHandler: ((event: { payload: unknown }) => void) | undefined;

vi.mock('../platform/ipc', () => ({
  listen: vi.fn(async (_name: string, handler: (event: { payload: unknown }) => void) => {
    eventHandler = handler;
    return () => {};
  }),
}));

import { startSidebarSettle } from './sidebarSettle';
import { sidebarNodesAtom, setSidebarTreeAtom } from '../state/sidebar';

const store = getDefaultStore();

function counts(duplicates: number) {
  return {
    active: 1,
    inbox: 1,
    trash: 0,
    uncategorized: 0,
    untagged: 0,
    duplicates,
  };
}

describe('sidebar runtime settling', () => {
  beforeEach(() => {
    eventHandler = undefined;
    store.set(setSidebarTreeAtom, {
      epoch: 1,
      nodes: [{
        id: 'system:duplicates',
        kind: 'system',
        parent_id: null,
        name: 'Duplicates',
        icon: null,
        color: null,
        sort_order: 6,
        count: 1,
        freshness: 'exact',
        selectable: true,
        expanded_by_default: false,
        meta: null,
      }],
    });
  });

  it('settles duplicate count to zero on Trash and restores it without rescanning', async () => {
    const stop = startSidebarSettle();
    await Promise.resolve();
    expect(eventHandler).toBeDefined();

    eventHandler!({
      payload: {
        origin: 'set_entity_status',
        changes: { status_changed: true },
        sidebar_counts: counts(0),
      },
    });
    expect(store.get(sidebarNodesAtom).find((node) => node.id === 'system:duplicates')?.count)
      .toBe(0);

    eventHandler!({
      payload: {
        origin: 'set_entity_status',
        changes: { status_changed: true },
        sidebar_counts: counts(1),
      },
    });
    expect(store.get(sidebarNodesAtom).find((node) => node.id === 'system:duplicates')?.count)
      .toBe(1);

    stop();
  });
});
