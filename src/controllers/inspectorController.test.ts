import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock('../platform/ipc', () => ({ invoke: invokeMock }));

import { cancelInspectorLoad, loadInspectorData } from './inspectorController';
import {
  displayedInspectorItemDetailsAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
} from '../state/inspector';

const store = getDefaultStore();
const details = {
  item_id: 2,
  kind: 'media',
  lifecycle: 'active',
  label: null,
  cover_media_item_id: null,
  folder_ids: [],
  media: [],
  aggregate_tags: [],
  revision: 1,
};

describe('inspectorController delayed loading', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invokeMock.mockReset();
    store.set(displayedInspectorTargetAtom, { kind: 'item', itemId: 1 });
    store.set(displayedInspectorItemDetailsAtom, { item_id: 1 } as never);
    store.set(inspectorLoadingAtom, false);
    store.set(inspectorErrorAtom, null);
  });

  afterEach(() => {
    cancelInspectorLoad();
    vi.useRealTimers();
  });

  it('keeps the previous inspector for 250ms, then shows loading until one atomic swap', async () => {
    let resolveDetails: ((value: typeof details) => void) | undefined;
    invokeMock.mockReturnValue(new Promise((resolve) => { resolveDetails = resolve; }));

    const request = loadInspectorData(2);
    await vi.advanceTimersByTimeAsync(249);
    expect(store.get(displayedInspectorTargetAtom)).toEqual({ kind: 'item', itemId: 1 });
    expect(store.get(inspectorLoadingAtom)).toBe(false);

    await vi.advanceTimersByTimeAsync(1);
    expect(store.get(displayedInspectorTargetAtom)).toEqual({ kind: 'item', itemId: 2 });
    expect(store.get(displayedInspectorItemDetailsAtom)).toBeNull();
    expect(store.get(inspectorLoadingAtom)).toBe(true);

    resolveDetails?.(details);
    await request;
    expect(store.get(displayedInspectorItemDetailsAtom)).toEqual(details);
    expect(store.get(inspectorLoadingAtom)).toBe(false);
  });

  it('never flashes loading when details arrive within the threshold', async () => {
    invokeMock.mockResolvedValue(details);

    await loadInspectorData(2);
    await vi.advanceTimersByTimeAsync(250);

    expect(store.get(displayedInspectorTargetAtom)).toEqual({ kind: 'item', itemId: 2 });
    expect(store.get(displayedInspectorItemDetailsAtom)).toEqual(details);
    expect(store.get(inspectorLoadingAtom)).toBe(false);
  });
});
