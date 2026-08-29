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
  root: {
    root_id: 2,
    stable_key: 'root-2',
    kind: 'media',
    name: 'Item 2',
    notes: null,
    source_urls: [],
    cover_media_id: 2,
    imported_at_ms: 1,
    captured_at_ms: null,
    modified_at_ms: 1,
    media_count: 0,
    total_size_bytes: 0,
  },
  lifecycle: 'active',
  rating: 'unrated',
  folder_ids: [],
  tag_ids: [],
  media: [],
  revision: 1,
};

describe('inspectorController delayed loading', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invokeMock.mockReset();
    store.set(displayedInspectorTargetAtom, { kind: 'item', itemId: 1 });
    store.set(displayedInspectorItemDetailsAtom, { root: { root_id: 1 } } as never);
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
    expect(store.get(displayedInspectorItemDetailsAtom)).toEqual({ ...details, resolved_tag_records: [] });
    expect(store.get(inspectorLoadingAtom)).toBe(false);
  });

  it('never flashes loading when details arrive within the threshold', async () => {
    invokeMock.mockResolvedValue(details);

    await loadInspectorData(2);
    await vi.advanceTimersByTimeAsync(250);

    expect(store.get(displayedInspectorTargetAtom)).toEqual({ kind: 'item', itemId: 2 });
    expect(store.get(displayedInspectorItemDetailsAtom)).toEqual({ ...details, resolved_tag_records: [] });
    expect(store.get(inspectorLoadingAtom)).toBe(false);
  });

  it('waits for tag labels before committing a selected item', async () => {
    const taggedDetails = { ...details, tag_ids: [9] };
    const tag = {
      tag_id: 9,
      namespace_id: 1,
      namespace: 'creator',
      subname: 'alice',
      active_count: 1,
      assignment_count: 1,
    };
    let resolveTags: ((value: typeof tag[]) => void) | undefined;
    invokeMock.mockImplementation((command: string) => {
      if (command === 'items.details') return Promise.resolve(taggedDetails);
      if (command === 'tags.get_many') return new Promise((resolve) => { resolveTags = resolve; });
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });

    const request = loadInspectorData(2);
    await vi.advanceTimersByTimeAsync(0);

    expect(store.get(displayedInspectorTargetAtom)).toEqual({ kind: 'item', itemId: 1 });
    expect(store.get(displayedInspectorItemDetailsAtom)).not.toMatchObject({ root: { root_id: 2 } });
    expect(resolveTags).toBeTypeOf('function');

    resolveTags!([tag]);
    await request;

    expect(invokeMock).toHaveBeenCalledWith('tags.get_many', { tag_ids: [9] });
    expect(store.get(displayedInspectorItemDetailsAtom)).toEqual({
      ...taggedDetails,
      resolved_tag_records: [tag],
    });
  });
});
