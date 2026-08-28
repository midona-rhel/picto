import { act, cleanup, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { getDefaultStore } from 'jotai';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

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

const store = getDefaultStore();
const itemDetails = {
  item_id: 1,
  kind: 'media',
  lifecycle: 'active',
  label: 'Previous item',
  cover_media_item_id: null,
  folder_ids: [],
  aggregate_tags: [],
  revision: 1,
  media: [{
    media_item_id: 1,
    file_hash: 'file-1',
    mime_type: 'image/jpeg',
    dominant_color_hex: null,
    dominant_colors: [],
    size_bytes: 100,
    pixel_width: 20,
    pixel_height: 10,
    duration_ms: null,
    frame_count: null,
    name: 'Previous item',
    notes: null,
    rating: null,
    source_urls: [],
    captured_at: null,
    imported_at: '2026-01-02',
    position: 0,
    tags: [],
  }],
};

const summary = {
  total_count: 2,
  selected_count: 2,
  sample_hashes: ['file-1', 'file-2'],
  shared_tags: [],
  top_tags: [],
  shared_folders: [],
  shared_notes: null,
  has_notes: false,
  shared_source_urls: [],
  has_source_urls: false,
  stats: {
    total_size_bytes: 300,
    media_count: 2,
    all_media_are_images: true,
    rating_stats: { min: null, max: null, shared: null },
  },
  revision: 2,
};

describe('Inspector selection transition', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(entityMutations.getTargetSelectionSummary).mockReset();
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
    cleanup();
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
    expect(screen.getByText('2 items selected')).toBeInTheDocument();
    expect(document.querySelector('[data-inspector-core-property="Size"]')).toHaveTextContent('300 B');
    expect(document.querySelector('[data-inspector-summary-loading]')).not.toBeInTheDocument();
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
    expect(screen.getByText('2 items selected')).toBeInTheDocument();
    expect(document.querySelector('[data-inspector-summary-loading]')).not.toBeInTheDocument();
  });
});
