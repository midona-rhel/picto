import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ItemFilters } from '../../shared/lib/itemFilters';
import { gridFilterLockedAtom, gridFilterToolbarOpenAtom, gridSessionAtom } from '../../state/grid';
import { countActiveGridFilters, GridFilterToolbar } from './GridFilterMenu';
import { createEmptyItemFilters } from '../../shared/lib/itemFilters';
import { gridController } from '../../controllers/gridController';
import { tagSelectPortalAtom } from '../../state/portals';

vi.mock('../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: unknown }) => children,
}));

const emptyFilters: ItemFilters = createEmptyItemFilters();

describe('countActiveGridFilters', () => {
  it('counts each active replacement filter represented by the toolbar badge', () => {
    expect(countActiveGridFilters({
      ...emptyFilters,
      ratings: [3],
      include_mime_types: ['image/jpeg'],
      color_hex: '#123456',
      include_tags: [{ tag_id: 1, name: 'favorite' }],
      exclude_tags: [{ tag_id: 2, name: 'spoiler' }],
      include_folder_ids: [1],
    })).toBe(6);
  });

  it('ignores empty filter values and independent text search', () => {
    expect(countActiveGridFilters({ ...emptyFilters, text: 'portrait' })).toBe(0);
  });

  it('counts each applicable range or presence family once', () => {
    expect(countActiveGridFilters({
      ...emptyFilters,
      imported_after: '2026-01-01T00:00:00Z',
      imported_before: '2026-02-01T00:00:00Z',
      min_duration_ms: 1000n,
      max_duration_ms: 5000n,
      min_width: 800n,
      max_height: 1200n,
      notes_present: true,
      notes_contains: 'caption',
      source_url_present: false,
    })).toBe(5);
  });
});

describe('GridFilterToolbar', () => {
  afterEach(() => {
    cleanup();
    window.localStorage.removeItem('picto:grid:pinned-filters');
    window.localStorage.removeItem('picto:grid:saved-filters');
    getDefaultStore().set(gridFilterToolbarOpenAtom, false);
    getDefaultStore().set(gridFilterLockedAtom, false);
    getDefaultStore().set(tagSelectPortalAtom, { open: false, anchor: null });
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it('renders default filters as a dedicated pinned row', () => {
    const store = getDefaultStore();
    store.set(gridFilterToolbarOpenAtom, true);
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), filters: emptyFilters });
    render(<GridFilterToolbar />);

    for (const label of ['Color', 'Tags', 'Folders', 'Rating', 'Type']) {
      expect(screen.getByRole('button', { name: label })).toBeTruthy();
    }
    expect(screen.getByRole('button', { name: 'Saved filters' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Lock filters' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Clear filters' })).toBeDisabled();
    fireEvent.click(screen.getByLabelText('Add filter'));
    for (const label of ['Date Imported', 'Date Modified', 'Resolution', 'Duration', 'File Size', 'Notes', 'URL']) {
      expect(screen.getByText(label)).toBeTruthy();
    }
  });

  it('locks filter navigation and clears every active filter through the right actions', () => {
    const store = getDefaultStore();
    store.set(gridFilterToolbarOpenAtom, true);
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      filters: { ...emptyFilters, include_tags: [{ tag_id: 1, name: 'favorite' }], ratings: [5] },
    });
    const setFilters = vi.spyOn(gridController, 'setFilters').mockImplementation(() => {});
    render(<GridFilterToolbar />);

    fireEvent.click(screen.getByRole('button', { name: 'Lock filters' }));
    expect(store.get(gridFilterLockedAtom)).toBe(true);
    expect(screen.getByRole('button', { name: 'Unlock filters' })).toHaveAttribute('aria-pressed', 'true');

    fireEvent.click(screen.getByRole('button', { name: 'Clear filters' }));
    expect(setFilters).toHaveBeenLastCalledWith(emptyFilters);
  });

  it('saves and reapplies a named filter without losing bigint ranges', () => {
    const store = getDefaultStore();
    const active = { ...emptyFilters, include_tags: [{ tag_id: 1, name: 'favorite' }], min_size_bytes: 2_000_000n };
    store.set(gridFilterToolbarOpenAtom, true);
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), filters: active });
    const setFilters = vi.spyOn(gridController, 'setFilters').mockImplementation(() => {});
    render(<GridFilterToolbar />);

    fireEvent.click(screen.getByRole('button', { name: 'Saved filters' }));
    fireEvent.change(screen.getByLabelText('Saved filter name'), { target: { value: 'Large favorites' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    fireEvent.click(screen.getByText('Large favorites'));

    expect(setFilters).toHaveBeenLastCalledWith(expect.objectContaining({
      include_tags: [{ tag_id: 1, name: 'favorite' }],
      min_size_bytes: 2_000_000n,
    }));
  });

  it('offers every applicable data-backed filter and commits numeric values in canonical units', () => {
    vi.useFakeTimers();
    window.localStorage.setItem('picto:grid:pinned-filters', JSON.stringify(['size']));
    const store = getDefaultStore();
    store.set(gridFilterToolbarOpenAtom, true);
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), filters: emptyFilters });
    const setFilters = vi.spyOn(gridController, 'setFilters').mockImplementation(() => {});
    render(<GridFilterToolbar />);

    fireEvent.click(screen.getByRole('button', { name: 'File Size' }));
    const minimum = screen.getByLabelText('Minimum');
    fireEvent.change(minimum, { target: { value: '2' } });
    act(() => vi.advanceTimersByTime(300));

    expect(setFilters).toHaveBeenLastCalledWith(expect.objectContaining({ min_size_bytes: 2_000_000n }));
  });

  it('uses presence plus keyword semantics for notes', () => {
    vi.useFakeTimers();
    window.localStorage.setItem('picto:grid:pinned-filters', JSON.stringify(['notes']));
    const store = getDefaultStore();
    store.set(gridFilterToolbarOpenAtom, true);
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      filters: { ...emptyFilters, notes_present: true },
    });
    const setFilters = vi.spyOn(gridController, 'setFilters').mockImplementation(() => {});
    render(<GridFilterToolbar />);

    fireEvent.click(screen.getByRole('button', { name: 'Has Notes' }));
    const keyword = screen.getByLabelText('Search notes');
    fireEvent.change(keyword, { target: { value: 'caption' } });
    act(() => vi.advanceTimersByTime(300));

    expect(setFilters).toHaveBeenLastCalledWith(expect.objectContaining({
      notes_present: true,
      notes_contains: 'caption',
    }));
  });

  it('commits date presets as canonical half-open ranges', () => {
    window.localStorage.setItem('picto:grid:pinned-filters', JSON.stringify(['imported']));
    const store = getDefaultStore();
    store.set(gridFilterToolbarOpenAtom, true);
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), filters: emptyFilters });
    const setFilters = vi.spyOn(gridController, 'setFilters').mockImplementation(() => {});
    render(<GridFilterToolbar />);

    fireEvent.click(screen.getByRole('button', { name: 'Date Imported' }));
    fireEvent.click(screen.getByText('Last 7 Days'));

    expect(setFilters).toHaveBeenLastCalledWith(expect.objectContaining({
      imported_after: expect.stringMatching(/^\d{4}-\d{2}-\d{2}T00:00:00\.000Z$/),
      imported_before: expect.stringMatching(/^\d{4}-\d{2}-\d{2}T00:00:00\.000Z$/),
    }));
  });

  it('anchors tag filtering directly below the tag control', () => {
    const store = getDefaultStore();
    store.set(gridFilterToolbarOpenAtom, true);
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), filters: emptyFilters });
    render(<GridFilterToolbar />);

    const tagControl = screen.getByRole('button', { name: 'Tags' });
    vi.spyOn(tagControl, 'getBoundingClientRect').mockReturnValue({
      left: 120, top: 20, right: 180, bottom: 44, width: 60, height: 24,
      x: 120, y: 20, toJSON: () => ({}),
    });
    fireEvent.click(tagControl);

    expect(store.get(tagSelectPortalAtom)).toMatchObject({
      open: true,
      anchor: { x: 120, y: 48 },
      anchorPlacement: 'below',
      filterMatchMode: 'any',
    });
  });

  it('uses the in-app color editor instead of the native picker', () => {
    const store = getDefaultStore();
    store.set(gridFilterToolbarOpenAtom, true);
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), filters: emptyFilters });
    const setFilters = vi.spyOn(gridController, 'setFilters').mockImplementation(() => {});
    render(<GridFilterToolbar />);

    fireEvent.click(screen.getByRole('button', { name: 'Color' }));
    expect(screen.getByLabelText('Color saturation and brightness')).toBeTruthy();
    expect(screen.getByLabelText('Color hue')).toBeTruthy();
    expect(screen.getByLabelText('Hex color')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '#FF2727' }));

    expect(setFilters).toHaveBeenLastCalledWith(expect.objectContaining({ color_hex: '#FF2727' }));
    expect(document.querySelector('input[type="color"]')).toBeNull();
  });

  it('maps the green hue position to green rather than the opposing purple hue', () => {
    vi.useFakeTimers();
    const store = getDefaultStore();
    store.set(gridFilterToolbarOpenAtom, true);
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      filters: { ...emptyFilters, color_hex: '#FF0000' },
    });
    const setFilters = vi.spyOn(gridController, 'setFilters').mockImplementation(() => {});
    render(<GridFilterToolbar />);

    fireEvent.click(screen.getByRole('button', { name: '#FF0000' }));
    fireEvent.change(screen.getByLabelText('Color hue'), { target: { value: '120' } });
    act(() => vi.advanceTimersByTime(100));

    expect(setFilters).toHaveBeenLastCalledWith(expect.objectContaining({ color_hex: '#00FF00' }));
  });
});
