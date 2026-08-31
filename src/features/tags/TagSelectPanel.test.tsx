import { act, fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { createStore, Provider } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { tagSelectPortalAtom } from '../../state/portals';
import { displayedInspectorItemDetailsAtom } from '../../state/inspector';
import { TagSelectPanel } from './TagSelectPanel';
import styles from './TagSelectPanel.module.css';

const mocks = vi.hoisted(() => ({
  getPaginated: vi.fn(),
  getNamespaceSummary: vi.fn(),
}));

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 28,
    getVirtualItems: () => Array.from({ length: count }, (_, index) => ({
      index,
      key: index,
      size: 28,
      start: index * 28,
    })),
    scrollToIndex: vi.fn(),
  }),
}));
vi.mock('../../controllers/tagsController', () => ({ tagsController: mocks }));
vi.mock('./tagPreferences', () => ({
  useTagPreferences: () => ({ showTagGroups: true, showTagPrefixes: false, starredTags: [], tagGroupColors: {} }),
  setTagStarred: vi.fn(),
  replaceStarredTag: vi.fn(),
}));

describe('TagSelectPanel assignment', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getPaginated.mockResolvedValue({
      tags: [
        { tag_id: 1, namespace_id: 1, namespace: 'creator', subname: 'alice', active_count: 4, assignment_count: 4 },
        { tag_id: 2, namespace_id: 1, namespace: 'creator', subname: 'bob', active_count: 2, assignment_count: 2 },
      ],
      next_cursor: null,
      revision: 1,
    });
    mocks.getNamespaceSummary.mockResolvedValue([{ namespace_id: 1, name: 'creator', tag_count: 2 }]);
  });

  it('focuses search from the full visible header row', async () => {
    const store = createStore();
    store.set(tagSelectPortalAtom, { open: true, selectedTags: [] });

    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });
    const search = screen.getByPlaceholderText('Search...');

    fireEvent.mouseDown(search.parentElement!);

    expect(search).toHaveFocus();
  });

  it('preserves the established geometry with and without the group rail', async () => {
    const store = createStore();
    store.set(tagSelectPortalAtom, { open: true, selectedTags: [] });

    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });

    const panel = document.querySelector<HTMLElement>('[data-overlay-shell]')!;
    expect(panel.style.width).toBe('540px');
    expect(panel.style.height).toBe('480px');
    const sidebarToggle = screen.getByRole('button', { name: 'Hide sidebar' });
    expect(sidebarToggle.className).toContain('pinBtnActive');

    fireEvent.click(sidebarToggle);
    expect(panel.style.width).toBe('340px');
    expect(panel.style.height).toBe('480px');
    expect(screen.getByRole('button', { name: 'Show sidebar' }).className).not.toContain('pinBtnActive');
  });

  it('supports the advertised move, select, and view-switch keys', async () => {
    const store = createStore();
    const onApplyTags = vi.fn();
    store.set(tagSelectPortalAtom, { open: true, selectedTags: [], onApplyTags });
    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });
    const search = screen.getByPlaceholderText('Search...');
    await screen.findByText('alice');

    fireEvent.keyDown(search, { key: 'ArrowDown' });
    fireEvent.keyDown(search, { key: 'Enter' });
    expect(onApplyTags).toHaveBeenLastCalledWith(['creator:alice']);

    await act(async () => { fireEvent.keyDown(search, { key: 'Tab' }); await Promise.resolve(); });
    expect(screen.getByText('creator').closest('[class*="sidebarItem"]')).toHaveClass(styles.sidebarItemActive);
    await act(async () => { fireEvent.keyDown(search, { key: 'Tab' }); await Promise.resolve(); });
    expect(screen.getByText('Selected').closest('[class*="sidebarItem"]')).toHaveClass(styles.sidebarItemActive);
    await act(async () => { fireEvent.keyDown(search, { key: 'Tab' }); await Promise.resolve(); });
    expect(screen.getByText('Starred').closest('[class*="sidebarItem"]')).toHaveClass(styles.sidebarItemActive);
    await act(async () => { fireEvent.keyDown(search, { key: 'Tab' }); await Promise.resolve(); });
    expect(screen.getByText('All').closest('[class*="sidebarItem"]')).toHaveClass(styles.sidebarItemActive);
  });

  it('does not cycle hidden tag views', async () => {
    const store = createStore();
    store.set(tagSelectPortalAtom, { open: true, selectedTags: [] });
    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });
    const search = screen.getByPlaceholderText('Search...');
    fireEvent.click(screen.getByRole('button', { name: 'Hide sidebar' }));

    expect(fireEvent.keyDown(search, { key: 'Tab' })).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Show sidebar' }));
    expect(screen.getByText('All').closest('[class*="sidebarItem"]')).toHaveClass(styles.sidebarItemActive);
  });

  it('updates multi-tag assignment on each click without an Apply step', async () => {
    const store = createStore();
    const onApplyTags = vi.fn();
    store.set(tagSelectPortalAtom, {
      open: true,
      selectedTags: [],
      onApplyTags,
    });

    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });

    expect(screen.queryByRole('button', { name: /Apply/ })).not.toBeInTheDocument();
    const aliceRow = (await screen.findByText('alice')).closest('[data-tag-index]')!;
    const aliceBookmark = aliceRow.querySelector('.tabler-icon-bookmark');
    expect(aliceBookmark).toHaveAttribute('fill', 'none');
    expect(aliceBookmark).toHaveAttribute('fill-opacity', '0');
    fireEvent.click(aliceRow);
    expect(aliceBookmark).toHaveAttribute('fill', 'currentColor');
    expect(aliceBookmark).toHaveAttribute('fill-opacity', '1');
    expect(onApplyTags).toHaveBeenLastCalledWith(['creator:alice']);
    fireEvent.click(screen.getByText('bob'));
    expect(onApplyTags).toHaveBeenLastCalledWith(['creator:alice', 'creator:bob']);
    fireEvent.click(screen.getByText('alice'));
    expect(onApplyTags).toHaveBeenLastCalledWith(['creator:bob']);
  });

  it('pins the opening selection at the top without reflowing after edits', async () => {
    const store = createStore();
    const onApplyTags = vi.fn();
    store.set(tagSelectPortalAtom, {
      open: true,
      selectedTags: ['creator:bob'],
      onApplyTags,
    });

    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });

    await screen.findByText('alice');
    const rows = () => [...document.querySelectorAll<HTMLElement>('[data-tag-index]')];
    expect(rows()[0]).toHaveTextContent('bob');
    expect(rows()[0]).toHaveClass(styles.tagRowSelected);
    expect(rows()[0].querySelector('.tabler-icon-bookmark')).toHaveAttribute('fill', 'currentColor');

    fireEvent.click(screen.getByText('alice'));
    expect(rows().map((row) => row.textContent)).toEqual(['bob(2)', 'alice(4)']);

    fireEvent.click(screen.getByText('bob'));
    expect(rows().map((row) => row.textContent)).toEqual(['bob(2)', 'alice(4)']);
    expect(rows()[0]).not.toHaveClass(styles.tagRowSelected);
  });

  it('uses the inspector tag snapshot when the portal opens', async () => {
    const store = createStore();
    store.set(displayedInspectorItemDetailsAtom, {
      root: {
        root_id: 9,
        stable_key: 'item:9',
        kind: 'media',
        name: 'Nine',
        notes: null,
        source_urls: [],
        cover_media_id: 90,
        imported_at_ms: 1,
        captured_at_ms: null,
        modified_at_ms: 1,
        media_count: 1,
        total_size_bytes: 1,
      },
      lifecycle: 'active',
      rating: 'unrated',
      folder_ids: [],
      tag_ids: [2],
      media: [],
      revision: 1,
      resolved_tag_records: [
        { tag_id: 2, namespace_id: 1, namespace: 'creator', subname: 'bob', active_count: 2, assignment_count: 2 },
      ],
    });
    store.set(tagSelectPortalAtom, { open: true });

    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });

    await screen.findByText('alice');
    const rows = [...document.querySelectorAll<HTMLElement>('[data-tag-index]')];
    expect(rows[0]).toHaveTextContent('bob');
    expect(rows[0]).toHaveClass(styles.tagRowSelected);
  });

  it('creates and assigns a missing tag directly from search', async () => {
    const store = createStore();
    const onApplyTags = vi.fn();
    mocks.getPaginated.mockResolvedValue({
      tags: [
        { tag_id: 1, namespace_id: 1, namespace: 'creator', subname: 'alice', active_count: 4, assignment_count: 4 },
        { tag_id: 3, namespace_id: 2, namespace: 'female', subname: 'alice', active_count: 4, assignment_count: 4 },
        { tag_id: 2, namespace_id: 1, namespace: 'creator', subname: 'bob', active_count: 2, assignment_count: 2 },
      ],
      next_cursor: null,
      revision: 1,
    });
    store.set(tagSelectPortalAtom, { open: true, selectedTags: [], onApplyTags });

    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });

    fireEvent.change(screen.getByPlaceholderText('Search...'), { target: { value: 'creator:a' } });
    const createLabel = await screen.findByText('creator:a');
    const createRow = createLabel.closest('[data-tag-index]');
    expect(createRow).toHaveTextContent('Create "creator:a"');
    expect(createRow).toHaveClass(styles.createTagRow);
    expect(createRow?.parentElement?.firstElementChild).toBe(createRow);
    expect((await screen.findByText('alice')).closest('[data-tag-index]')).toHaveAttribute('data-tag-index', '1');
    expect(document.querySelectorAll('[data-tag-index]')).toHaveLength(2);
    fireEvent.click(createLabel);

    expect(onApplyTags).toHaveBeenLastCalledWith(['creator:a']);
  });

  it('keeps the settled tag view until the current backend search returns', async () => {
    const store = createStore();
    store.set(tagSelectPortalAtom, {
      open: true,
      selectedTags: [],
      onApplyTagFilter: vi.fn(),
    });

    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });

    let resolveSearch!: (value: { tags: []; next_cursor: null; revision: number }) => void;
    mocks.getPaginated.mockImplementationOnce(() => new Promise((resolve) => {
      resolveSearch = resolve;
    }));

    fireEvent.change(screen.getByPlaceholderText('Search...'), { target: { value: 'missing' } });
    expect(screen.getByText('alice')).toBeInTheDocument();
    expect(screen.queryByText('No tags found')).not.toBeInTheDocument();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 170));
      resolveSearch({ tags: [], next_cursor: null, revision: 2 });
      await Promise.resolve();
    });

    expect(screen.getByText('No tags found')).toBeInTheDocument();
    expect(screen.queryByText('alice')).not.toBeInTheDocument();
  });

  it('updates filters and matching mode immediately', async () => {
    const store = createStore();
    const onApplyTagFilter = vi.fn();
    store.set(tagSelectPortalAtom, {
      open: true,
      selectedTags: ['creator:alice'],
      excludedTags: [],
      filterMatchMode: 'any',
      onApplyTagFilter,
    });

    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });

    fireEvent.click(await screen.findByText('bob'));
    expect(onApplyTagFilter).toHaveBeenLastCalledWith(
      [
        { tag_id: 1, name: 'creator:alice' },
        { tag_id: 2, name: 'creator:bob' },
      ],
      [],
      'any',
    );
    expect(screen.getByText('L-Click')).toBeInTheDocument();
    expect(screen.getByText('R-Click')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Tag picker settings' })).not.toBeInTheDocument();
    expect(document.querySelector('.tabler-icon-pin')).not.toBeInTheDocument();
    expect(document.querySelector<HTMLElement>('[class*="tagGrid"]')?.style.gridTemplateColumns)
      .toBe('repeat(1, minmax(0, 1fr))');
    expect(screen.queryByRole('button', { name: /Apply/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Match all' }));
    expect(onApplyTagFilter).toHaveBeenLastCalledWith(
      [
        { tag_id: 1, name: 'creator:alice' },
        { tag_id: 2, name: 'creator:bob' },
      ],
      [],
      'all',
    );
  });

  it('uses the compact multi-column grid and keeps a list layout option', async () => {
    const store = createStore();
    store.set(tagSelectPortalAtom, { open: true, selectedTags: [] });

    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });

    await screen.findByText('alice');
    expect(document.querySelector('[class*="tagGrid"]')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Tag picker settings' }));
    expect(screen.getByRole('button', { name: 'Grid tags' })).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'List tags' }));
    expect(screen.getByRole('button', { name: 'List tags' })).toHaveAttribute('aria-pressed', 'true');
  });
});
