import { act, cleanup, render, screen } from '@testing-library/react';
import { Profiler } from 'react';
import { MantineProvider } from '@mantine/core';
import { getDefaultStore } from 'jotai';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invalidationMocks = vi.hoisted(() => ({
  callbacks: new Set<(payload: { revision: number; resources: string[]; item_ids: number[] }) => void>(),
  invoke: vi.fn(),
}));
vi.mock('../../platform/ipc', () => ({ invoke: invalidationMocks.invoke }));
vi.mock('../../runtime/libraryInvalidation', () => ({
  libraryInvalidation: {
    register: (_resource: string, callback: (payload: { revision: number; resources: string[]; item_ids: number[] }) => void) => {
      invalidationMocks.callbacks.add(callback);
      return () => invalidationMocks.callbacks.delete(callback);
    },
  },
}));

vi.mock('../../controllers/entityMutations', () => ({
  getTargetSelectionSummary: vi.fn(),
  removeItemFromFolder: vi.fn(),
  removeItemTags: vi.fn(),
  removeTargetTags: vi.fn(),
  setItemName: vi.fn(),
  setItemNotes: vi.fn(),
  setItemRating: vi.fn(),
  setItemSourceUrls: vi.fn(),
  setTargetNotes: vi.fn(),
  setTargetRating: vi.fn(),
  setTargetSourceUrls: vi.fn(),
  updateTargetFolderMembership: vi.fn(),
}));

const tagMocks = vi.hoisted(() => ({
  getById: vi.fn(),
  getNamespaceSummary: vi.fn(),
}));

vi.mock('../../controllers/tagsController', () => ({
  tagsController: tagMocks,
}));

import * as entityMutations from '../../controllers/entityMutations';
import { currentGridQueryAtom, gridSessionAtom } from '../../state/grid';
import {
  displayedInspectorItemDetailsAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  inspectorPinnedAtom,
} from '../../state/inspector';
import { emptyGridSelection, gridSelectionAtom } from '../../state/selection';
import { Inspector } from './Inspector';
import { startInspectorSettle } from '../../runtime/inspectorSettle';
import { cancelInspectorLoad } from '../../controllers/inspectorController';

const store = getDefaultStore();
let stopInspector: (() => void) | undefined;
const itemDetails = {
  root: {
    root_id: 1,
    stable_key: 'root-1',
    kind: 'media',
    name: 'Previous item',
    notes: null,
    source_urls: [],
    cover_media_id: 1,
    imported_at_ms: Date.parse('2026-01-02'),
    captured_at_ms: null,
    modified_at_ms: Date.parse('2026-01-02'),
    media_count: 1,
    total_size_bytes: 100,
  },
  lifecycle: 'active',
  rating: 'unrated',
  folder_ids: [],
  tag_ids: [],
  revision: 1,
  media: [{
    media_id: 1,
    media_name: 'Previous item',
    file_id: 1,
    file_path: '/tmp/file-1.jpg',
    facts: {
      mime: 'image/jpeg',
      size_bytes: 100,
      width: 20,
      height: 10,
      duration_ms: null,
      frame_count: null,
      content_hash: 'file-1',
      perceptual_hash: null,
      palette: [],
    },
  }],
};

const summary = {
  total_count: 2,
  selected_count: 2,
  total_size_bytes: 300,
  media_count: 2,
  sample_hashes: ['file-1', 'file-2'],
  shared_tags: [] as number[],
  shared_folders: [] as number[],
  shared_rating: null,
  minimum_rating: null,
  maximum_rating: null,
  shared_notes: null,
  has_notes: false,
  shared_source_urls: null,
  has_source_urls: false,
  collection_candidates: [],
  all_selected_roots_have_images: true,
  revision: 2,
};

describe('Inspector selection transition', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invalidationMocks.invoke.mockReset();
    vi.mocked(entityMutations.getTargetSelectionSummary).mockReset();
    tagMocks.getById.mockReset();
    tagMocks.getById.mockResolvedValue([]);
    tagMocks.getNamespaceSummary.mockReset();
    tagMocks.getNamespaceSummary.mockResolvedValue([]);
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      active: true,
      totalCount: 2,
    });
    store.set(gridSelectionAtom, {
      ...emptyGridSelection(),
      mode: 'query_results',
      query: store.get(currentGridQueryAtom),
      queryTotalCount: 2,
    });
    store.set(displayedInspectorTargetAtom, { kind: 'item', itemId: 1 });
    store.set(displayedInspectorItemDetailsAtom, itemDetails as never);
    store.set(inspectorLoadingAtom, false);
    store.set(inspectorErrorAtom, null);
    store.set(inspectorPinnedAtom, false);
  });

  afterEach(() => {
    stopInspector?.();
    stopInspector = undefined;
    cancelInspectorLoad();
    cleanup();
    invalidationMocks.callbacks.clear();
    store.set(gridSelectionAtom, emptyGridSelection());
    store.set(displayedInspectorTargetAtom, { kind: 'none' });
    store.set(displayedInspectorItemDetailsAtom, null);
    vi.useRealTimers();
  });

  it('keeps the previous inspector until a fast summary is ready', async () => {
    let resolveSummary: ((value: typeof summary) => void) | undefined;
    vi.mocked(entityMutations.getTargetSelectionSummary).mockReturnValue(new Promise((resolve) => {
      resolveSummary = resolve;
    }) as never);
    render(<MantineProvider><Inspector /></MantineProvider>);

    expect(screen.getByText('Previous item')).toBeInTheDocument();
    expect(document.querySelector('[data-inspector-summary-loading]')).not.toBeInTheDocument();

    await act(async () => { resolveSummary?.(summary); });

    expect(screen.queryByText('Previous item')).not.toBeInTheDocument();
    expect(document.querySelector('[data-inspector-selection-count]')).toHaveTextContent('2 items selected');
    expect(document.querySelector('[data-inspector-preview-hash="file-2"]')).toHaveAttribute('data-inspector-stack-entering');
    expect(document.querySelector('[data-inspector-preview-hash="file-1"]')).toHaveStyle({ opacity: '1' });
    expect(document.querySelector('[data-inspector-preview-hash="file-1"] [class*="stackShade"]')).toHaveStyle({ opacity: '0.7' });
    expect(document.querySelector('[data-inspector-core-property="Size"]')).toHaveTextContent('300 B');
    expect(document.querySelector('[data-inspector-summary-loading]')).not.toBeInTheDocument();
  });

  it('never renders single-item properties while Undo and Redo refresh a multi-selection', async () => {
    vi.mocked(entityMutations.getTargetSelectionSummary).mockResolvedValue(summary as never);
    invalidationMocks.invoke.mockResolvedValue({ ...itemDetails, media: [] });
    const committedProperties: string[][] = [];
    render(<MantineProvider><Profiler id="inspector" onRender={() => {
      committedProperties.push(Array.from(document.querySelectorAll('[data-inspector-core-property]'),
        (node) => node.getAttribute('data-inspector-core-property')!));
    }}><Inspector /></Profiler></MantineProvider>);
    await act(async () => {});
    expect(document.querySelector('[data-inspector-selection-count]')).toHaveTextContent('2 items selected');
    committedProperties.length = 0;
    stopInspector = startInspectorSettle();

    for (const revision of [3, 4]) {
      let resolveSummary!: (value: typeof summary) => void;
      vi.mocked(entityMutations.getTargetSelectionSummary).mockReturnValue(new Promise((resolve) => {
        resolveSummary = resolve;
      }) as never);
      await act(async () => {
        invalidationMocks.callbacks.forEach((callback) => callback({ revision, resources: ['library'], item_ids: [1, 2] }));
      });
      await act(async () => { await vi.advanceTimersByTimeAsync(300); });
      expect(document.querySelector('[data-inspector-selection-count]')).toHaveTextContent('2 items selected');
      expect(document.querySelector('[data-inspector-core-property="Dimensions"]')).not.toBeInTheDocument();
      await act(async () => { resolveSummary({ ...summary, revision }); });
    }

    expect(committedProperties.length).toBeGreaterThan(0);
    expect(committedProperties.every((properties) => properties.join(',') === 'Media,Size')).toBe(true);
    expect(invalidationMocks.invoke).not.toHaveBeenCalledWith('items.details', expect.anything());
  });

  it('shows the loading presentation only after 250ms', async () => {
    let resolveSummary: ((value: typeof summary) => void) | undefined;
    vi.mocked(entityMutations.getTargetSelectionSummary).mockReturnValue(new Promise((resolve) => {
      resolveSummary = resolve;
    }) as never);
    render(<MantineProvider><Inspector /></MantineProvider>);

    act(() => { vi.advanceTimersByTime(249); });
    expect(screen.getByText('Previous item')).toBeInTheDocument();
    expect(document.querySelector('[data-inspector-summary-loading]')).not.toBeInTheDocument();

    act(() => { vi.advanceTimersByTime(1); });
    expect(screen.queryByText('Previous item')).not.toBeInTheDocument();
    expect(document.querySelectorAll('[data-inspector-summary-loading]').length).toBeGreaterThan(0);

    await act(async () => { resolveSummary?.(summary); });
    expect(document.querySelector('[data-inspector-selection-count]')).toHaveTextContent('2 items selected');
    expect(document.querySelector('[data-inspector-summary-loading]')).not.toBeInTheDocument();
  });

  it('keeps the previous inspector until shared tag labels are ready', async () => {
    let resolveSummary: ((value: typeof summary) => void) | undefined;
    let resolveTags: ((value: Array<Record<string, unknown>>) => void) | undefined;
    vi.mocked(entityMutations.getTargetSelectionSummary).mockReturnValue(new Promise((resolve) => {
      resolveSummary = resolve;
    }) as never);
    tagMocks.getById.mockReturnValue(new Promise((resolve) => { resolveTags = resolve; }));
    render(<MantineProvider><Inspector /></MantineProvider>);

    await act(async () => { resolveSummary?.({ ...summary, shared_tags: [9] }); });
    expect(screen.getByText('Previous item')).toBeInTheDocument();
    expect(tagMocks.getById).toHaveBeenCalledWith([9]);

    await act(async () => { resolveTags?.([{
      tag_id: 9,
      namespace_id: 1,
      namespace: 'creator',
      subname: 'alice',
      active_count: 2,
      assignment_count: 2,
    }]); });

    expect(screen.queryByText('Previous item')).not.toBeInTheDocument();
    expect(screen.getByText('alice')).toBeInTheDocument();
  });
});
