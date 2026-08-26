import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createStore, Provider } from 'jotai';
import type { CanonicalTagRecord } from '../../shared/types/canonical';
import { renderWithProviders } from '../../test/render';
import { TagManagerScreen, TagsToolbar } from './TagManagerScreen';

const mocks = vi.hoisted(() => ({
  getPaginated: vi.fn(),
  getNamespaceSummary: vi.fn(),
  getUnusedCount: vi.fn(),
  getRelations: vi.fn(),
  rename: vi.fn(),
  merge: vi.fn(),
  delete: vi.fn(),
  deleteUnused: vi.fn(),
  renameGroup: vi.fn(),
  deleteGroup: vi.fn(),
  setAlias: vi.fn(),
  setImplication: vi.fn(),
  registerInvalidation: vi.fn(),
  startInvalidation: vi.fn(),
  showTagManagerItems: vi.fn(),
  getSettings: vi.fn(),
  patchSettings: vi.fn(),
}));

vi.mock('../../controllers/tagsController', () => ({ tagsController: mocks }));
vi.mock('../../runtime/libraryInvalidation', () => ({
  libraryInvalidation: {
    register: mocks.registerInvalidation,
    start: mocks.startInvalidation,
  },
}));
vi.mock('../../controllers/gridNavigationController', () => ({ showTagManagerItems: mocks.showTagManagerItems }));
vi.mock('../../platform/settingsApi', () => ({
  getSettings: mocks.getSettings,
  patchSettings: mocks.patchSettings,
}));

const firstTag: CanonicalTagRecord = {
  tag_id: 1,
  namespace: 'character',
  subtag: 'alice',
  file_count: 4,
};
const zeroTag: CanonicalTagRecord = {
  tag_id: 2,
  namespace: 'character',
  subtag: 'unused',
  file_count: 0,
};
const nextTag: CanonicalTagRecord = {
  tag_id: 3,
  namespace: 'creator',
  subtag: 'bob',
  file_count: 2,
};
const staleTag: CanonicalTagRecord = {
  tag_id: 4,
  namespace: 'stale',
  subtag: 'wrong-result',
  file_count: 1,
};

let invalidateTags: (() => void) | undefined;

beforeEach(() => {
  vi.clearAllMocks();
  invalidateTags = undefined;
  mocks.getNamespaceSummary.mockResolvedValue([
    { namespace: 'character', count: 2 },
    { namespace: 'creator', count: 1 },
  ]);
  mocks.getRelations.mockResolvedValue({ aliases: [], implications: [] });
  mocks.getUnusedCount.mockResolvedValue(1);
  mocks.rename.mockResolvedValue(undefined);
  mocks.merge.mockResolvedValue(undefined);
  mocks.delete.mockResolvedValue(undefined);
  mocks.deleteUnused.mockResolvedValue(undefined);
  mocks.renameGroup.mockResolvedValue(undefined);
  mocks.deleteGroup.mockResolvedValue(undefined);
  mocks.setAlias.mockResolvedValue(undefined);
  mocks.setImplication.mockResolvedValue(undefined);
  mocks.getSettings.mockResolvedValue({ showTagGroups: true, starredTags: [] });
  mocks.patchSettings.mockResolvedValue({ revision: 1, resources: ['settings'], item_ids: [] });
  mocks.registerInvalidation.mockImplementation((_resource: string, handler: () => void) => {
    invalidateTags = handler;
    return vi.fn();
  });
  mocks.getPaginated.mockImplementation(async ({ cursor, search }: { cursor?: string | null; search?: string | null }) => {
    if (search) return { items: [nextTag], next_cursor: null };
    if (cursor) return { items: [nextTag], next_cursor: null };
    return { items: [firstTag, zeroTag], next_cursor: 'opaque-cursor' };
  });
});

async function renderScreen() {
  let result: ReturnType<typeof renderWithProviders>;
  await act(async () => {
    result = renderWithProviders(
      <Provider store={createStore()}>
        <TagsToolbar />
        <TagManagerScreen />
      </Provider>,
    );
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
  return result!;
}

async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}

async function interact(action: () => Promise<unknown>) {
  await act(async () => {
    await action();
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function setupUser() {
  const raw = userEvent.setup();
  return {
    click: (...args: Parameters<typeof raw.click>) => interact(() => raw.click(...args)),
    type: (...args: Parameters<typeof raw.type>) => interact(() => raw.type(...args)),
    clear: (...args: Parameters<typeof raw.clear>) => interact(() => raw.clear(...args)),
  };
}

describe('TagManagerScreen', () => {
  it('focuses the titlebar search from anywhere in its visible capsule', async () => {
    await renderScreen();
    const search = screen.getByRole('textbox', { name: 'Search tags' });

    fireEvent.mouseDown(search.parentElement!);

    expect(search).toHaveFocus();
  });

  it('browses zero-count tags and follows the opaque cursor', async () => {
    await renderScreen();

    expect(await screen.findByText('character:unused')).toBeInTheDocument();
    expect(await screen.findByText('creator:bob')).toBeInTheDocument();
    expect(mocks.getPaginated).toHaveBeenCalledWith({
      namespace: null,
      search: null,
      cursor: 'opaque-cursor',
      limit: 100,
    });
    expect(screen.queryByRole('button', { name: 'Load more tags' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Refresh tags' })).not.toBeInTheDocument();
    const groupRail = screen.getByRole('navigation', { name: 'Tag groups' });
    expect(groupRail).toHaveTextContent('Groups (2)');
    expect(within(groupRail).queryByRole('button', { name: /tag groups/i })).not.toBeInTheDocument();
  });

  it('resets the page and closes the editor when search or namespace changes', async () => {
    const user = setupUser();
    await renderScreen();
    await user.click(await screen.findByRole('button', { name: /alice/ }));
    await settle();
    expect(screen.getByRole('dialog', { name: 'Edit tag' })).toBeInTheDocument();

    const search = screen.getByRole('textbox', { name: 'Search tags' });
    await user.type(search, 'bob');
    await settle();
    await waitFor(() => expect(mocks.getPaginated).toHaveBeenLastCalledWith({
      namespace: null,
      search: 'bob',
      cursor: null,
      limit: 100,
    }));
    expect(screen.queryByRole('dialog', { name: 'Edit tag' })).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'creator 1' }));
    await settle();
    await waitFor(() => expect(mocks.getPaginated).toHaveBeenLastCalledWith({
      namespace: 'creator',
      search: 'bob',
      cursor: null,
      limit: 100,
    }));
  });

  it('keeps ungrouped tags in All without exposing a misleading General group', async () => {
    mocks.getNamespaceSummary.mockResolvedValue([
      { namespace: '', count: 2 },
      { namespace: 'creator', count: 1 },
    ]);
    await renderScreen();

    expect(screen.queryByRole('button', { name: 'general 2' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'All tags 3' })).toBeInTheDocument();
    await waitFor(() => expect(mocks.getPaginated).toHaveBeenCalledWith({
      namespace: null,
      search: null,
      cursor: null,
      limit: 100,
    }));
  });

  it('ignores a stale load-more response after a filter reset', async () => {
    const user = setupUser();
    let resolveStale: ((page: { items: CanonicalTagRecord[]; next_cursor: null }) => void) | undefined;
    mocks.getPaginated.mockImplementation(({ cursor, search }: { cursor?: string | null; search?: string | null }) => {
      if (cursor) return new Promise((resolve) => { resolveStale = resolve; });
      if (search) return Promise.resolve({ items: [nextTag], next_cursor: null });
      return Promise.resolve({ items: [firstTag, zeroTag], next_cursor: 'opaque-cursor' });
    });
    await renderScreen();

    await screen.findByText('character:unused');
    await waitFor(() => expect(resolveStale).toBeTypeOf('function'));
    await user.type(screen.getByRole('textbox', { name: 'Search tags' }), 'bob');
    await settle();
    await screen.findByText('creator:bob');

    await act(async () => {
      resolveStale?.({ items: [staleTag], next_cursor: null });
    });
    expect(screen.queryByText('wrong-result')).not.toBeInTheDocument();
  });

  it('opens the editor and calls rename, alias, and implication mutations', async () => {
    const user = setupUser();
    await renderScreen();
    await user.click(await screen.findByRole('button', { name: /alice/ }));
    await settle();

    const renameInput = screen.getByRole('textbox', { name: 'New tag name' });
    await user.clear(renameInput);
    await settle();
    await user.type(renameInput, 'character:alice-renamed');
    await settle();
    await user.click(screen.getByRole('button', { name: 'Rename' }));
    await settle();
    await waitFor(() => expect(mocks.rename).toHaveBeenCalledWith(1, 'character:alice-renamed'));

    await user.click(await screen.findByRole('button', { name: /alice/ }));
    await settle();
    await user.click(screen.getByRole('button', { name: 'Add alias' }));
    await settle();
    const aliasDialog = screen.getByRole('dialog', { name: 'Add existing alias' });
    await user.type(within(aliasDialog).getByPlaceholderText('Search existing tags'), 'bob');
    await settle();
    await user.click(await within(aliasDialog).findByRole('button', { name: /bob/ }));
    await settle();
    await waitFor(() => expect(mocks.setAlias).toHaveBeenCalledWith(3, 1));

    await user.click(screen.getByRole('button', { name: 'Add parent' }));
    await settle();
    const parentDialog = screen.getByRole('dialog', { name: 'Add existing parent' });
    await user.type(within(parentDialog).getByPlaceholderText('Search existing tags'), 'bob');
    await settle();
    await user.click(await within(parentDialog).findByRole('button', { name: /bob/ }));
    await settle();
    await waitFor(() => expect(mocks.setImplication).toHaveBeenCalledWith(1, 3, true));
    expect(mocks.getPaginated).not.toHaveBeenCalledWith(expect.objectContaining({ limit: 0 }));
  });

  it('refreshes namespace summaries after mutations and runtime tag facts', async () => {
    const user = setupUser();
    await renderScreen();
    await user.click(await screen.findByRole('button', { name: /alice/ }));
    await settle();
    const initialSummaryCalls = mocks.getNamespaceSummary.mock.calls.length;
    await user.click(screen.getByRole('button', { name: 'Delete' }));
    await settle();
    await user.click(screen.getByRole('button', { name: 'Delete tag' }));
    await settle();
    await waitFor(() => expect(mocks.delete).toHaveBeenCalledWith(1));
    await waitFor(() => expect(mocks.getNamespaceSummary.mock.calls.length).toBeGreaterThan(initialSummaryCalls));

    const summaryCallsBeforeEvent = mocks.getNamespaceSummary.mock.calls.length;
    await act(async () => {
      invalidateTags?.();
    });
    await waitFor(() => expect(mocks.getNamespaceSummary.mock.calls.length).toBeGreaterThan(summaryCallsBeforeEvent));
  });

  it('closes the editor after delete and merge', async () => {
    const user = setupUser();
    await renderScreen();
    await user.click(await screen.findByRole('button', { name: /alice/ }));
    await settle();
    await user.click(screen.getByRole('button', { name: 'Delete' }));
    await settle();
    await user.click(screen.getByRole('button', { name: 'Delete tag' }));
    await settle();
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Edit tag' })).not.toBeInTheDocument());

    await user.click(await screen.findByRole('button', { name: /alice/ }));
    await settle();
    await user.click(screen.getByRole('button', { name: 'Merge into...' }));
    await settle();
    const mergeDialog = screen.getByRole('dialog', { name: 'Merge into existing tag' });
    await user.type(within(mergeDialog).getByPlaceholderText('Search existing tags'), 'bob');
    await settle();
    await user.click(await within(mergeDialog).findByRole('button', { name: /bob/ }));
    await settle();
    await waitFor(() => expect(mocks.merge).toHaveBeenCalledWith(1, 'creator:bob'));
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Edit tag' })).not.toBeInTheDocument());
  });

  it('uses the shared tag context menu for every tag row', async () => {
    await renderScreen();
    const tag = await screen.findByRole('button', { name: /character:alice/ });

    fireEvent.contextMenu(tag, { clientX: 100, clientY: 100 });

    expect(await screen.findByRole('menu')).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Filter Items with This Tag' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Add to Starred' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Move to Group…' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Remove from this Group' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Edit Tag' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Merge Into…' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Delete Tag' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('menuitem', { name: 'Filter Items with This Tag' }));
    expect(mocks.showTagManagerItems).toHaveBeenCalledWith('character:alice');
  });

  it('moves and ungroups tags through the canonical rename operation', async () => {
    await renderScreen();
    const tag = await screen.findByRole('button', { name: /character:alice/ });

    fireEvent.contextMenu(tag, { clientX: 100, clientY: 100 });
    fireEvent.mouseEnter(screen.getByRole('menuitem', { name: 'Move to Group…' }));
    fireEvent.click(await screen.findByRole('menuitem', { name: 'creator' }));
    await waitFor(() => expect(mocks.rename).toHaveBeenCalledWith(1, 'creator:alice'));

    fireEvent.contextMenu(tag, { clientX: 100, clientY: 100 });
    fireEvent.click(screen.getByRole('menuitem', { name: 'Remove from this Group' }));
    await waitFor(() => expect(mocks.rename).toHaveBeenCalledWith(1, 'alice'));
  });

  it('offers only applicable group actions and renames the whole group', async () => {
    const user = setupUser();
    await renderScreen();
    const group = screen.getByRole('button', { name: 'character 2' });

    fireEvent.contextMenu(group, { clientX: 100, clientY: 100 });
    expect(screen.getByRole('menuitem', { name: 'Show Tags in Group' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Rename Group…' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Delete Group' })).toBeInTheDocument();
    expect(screen.queryByRole('menuitem', { name: /Delete Tag$/ })).not.toBeInTheDocument();

    await user.click(screen.getByRole('menuitem', { name: 'Rename Group…' }));
    const dialog = screen.getByRole('dialog', { name: 'Rename tag group' });
    const input = within(dialog).getByRole('textbox', { name: 'Tag group name' });
    await user.clear(input);
    await user.type(input, 'Cast Members');
    await user.click(within(dialog).getByRole('button', { name: 'Rename' }));

    await waitFor(() => expect(mocks.renameGroup).toHaveBeenCalledWith('character', 'cast_members'));
  });

  it('deletes a group without deleting its tags', async () => {
    const user = setupUser();
    await renderScreen();
    fireEvent.contextMenu(screen.getByRole('button', { name: 'character 2' }), {
      clientX: 100,
      clientY: 100,
    });
    await user.click(screen.getByRole('menuitem', { name: 'Delete Group' }));

    const dialog = screen.getByRole('dialog', { name: 'Delete tag group' });
    expect(dialog).toHaveTextContent('tags will move to General');
    expect(dialog).toHaveTextContent('no tags or media assignments will be deleted');
    await user.click(within(dialog).getByRole('button', { name: 'Delete group' }));

    await waitFor(() => expect(mocks.deleteGroup).toHaveBeenCalledWith('character'));
    expect(mocks.delete).not.toHaveBeenCalled();
  });

  it('deletes all truly unused tags through one confirmed mutation', async () => {
    const user = setupUser();
    await renderScreen();

    await user.click(await screen.findByRole('button', { name: 'Delete unused tags 1' }));
    const dialog = screen.getByRole('dialog', { name: 'Delete unused tags' });
    expect(dialog).toHaveTextContent('1 tag with no media assignments or relationships');
    await user.click(within(dialog).getByRole('button', { name: 'Delete unused tags' }));

    await waitFor(() => expect(mocks.deleteUnused).toHaveBeenCalledTimes(1));
  });
});
