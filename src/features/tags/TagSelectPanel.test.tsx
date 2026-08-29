import { act, fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { createStore, Provider } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { tagSelectPortalAtom } from '../../state/portals';
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
  useTagPreferences: () => ({ showTagGroups: true, starredTags: [], tagGroupColors: {} }),
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

    fireEvent.click(screen.getByRole('button', { name: 'Hide sidebar' }));
    expect(panel.style.width).toBe('340px');
    expect(panel.style.height).toBe('480px');
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
    fireEvent.click(await screen.findByText('alice'));
    expect(onApplyTags).toHaveBeenLastCalledWith(['creator:alice']);
    fireEvent.click(screen.getByText('bob'));
    expect(onApplyTags).toHaveBeenLastCalledWith(['creator:alice', 'creator:bob']);
    fireEvent.click(screen.getByText('alice'));
    expect(onApplyTags).toHaveBeenLastCalledWith(['creator:bob']);
  });

  it('creates and assigns a missing tag directly from search', async () => {
    const store = createStore();
    const onApplyTags = vi.fn();
    store.set(tagSelectPortalAtom, { open: true, selectedTags: [], onApplyTags });

    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });

    fireEvent.change(screen.getByPlaceholderText('Search...'), { target: { value: 'creator:carol' } });
    const createLabel = await screen.findByText('creator:carol');
    const createRow = createLabel.closest('[data-tag-index]');
    expect(createRow).toHaveTextContent('Create "creator:carol"');
    expect(createRow).toHaveClass(styles.createTagRow);
    fireEvent.click(createLabel);

    expect(onApplyTags).toHaveBeenLastCalledWith(['creator:carol']);
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
