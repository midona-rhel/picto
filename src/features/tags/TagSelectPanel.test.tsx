import { act, fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { createStore, Provider } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { tagSelectPortalAtom } from '../../state/portals';
import { TagSelectPanel } from './TagSelectPanel';

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
  useTagPreferences: () => ({ showTagGroups: true, starredTags: [] }),
  setTagStarred: vi.fn(),
  replaceStarredTag: vi.fn(),
}));

describe('TagSelectPanel assignment', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getPaginated.mockResolvedValue({
      items: [
        { tag_id: 1, namespace: 'creator', subtag: 'alice', file_count: 4 },
        { tag_id: 2, namespace: 'creator', subtag: 'bob', file_count: 2 },
      ],
      next_cursor: null,
    });
    mocks.getNamespaceSummary.mockResolvedValue([{ namespace: 'creator', count: 2 }]);
  });

  it('focuses search from the full visible header row', async () => {
    const store = createStore();
    store.set(tagSelectPortalAtom, { open: true, selectedTags: [] });

    await act(async () => {
      render(<MantineProvider><Provider store={store}><TagSelectPanel /></Provider></MantineProvider>);
      await Promise.resolve();
    });
    const search = screen.getByPlaceholderText('Search tags...');

    fireEvent.mouseDown(search.parentElement!);

    expect(search).toHaveFocus();
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
      ['creator:alice', 'creator:bob'],
      [],
      'any',
    );
    expect(screen.queryByRole('button', { name: /Apply/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Match all' }));
    expect(onApplyTagFilter).toHaveBeenLastCalledWith(
      ['creator:alice', 'creator:bob'],
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
