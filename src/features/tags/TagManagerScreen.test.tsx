import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createStore, Provider } from 'jotai';
import type { CanonicalTagRecord } from '../../shared/types/canonical';
import { TagManagerScreen, TagsToolbar } from './TagManagerScreen';

const mocks = vi.hoisted(() => ({
  getPaginated: vi.fn(),
  getNamespaceSummary: vi.fn(),
  getRelations: vi.fn(),
  rename: vi.fn(),
  merge: vi.fn(),
  delete: vi.fn(),
  setAlias: vi.fn(),
  setImplication: vi.fn(),
  listen: vi.fn(),
  unlisten: vi.fn(),
}));

vi.mock('../../controllers/tagsController', () => ({ tagsController: mocks }));
vi.mock('../../platform/ipc', () => ({ listen: mocks.listen }));

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

let stateEvent: ((event: { payload: { changes: Record<string, unknown> } }) => void) | undefined;

beforeEach(() => {
  vi.clearAllMocks();
  stateEvent = undefined;
  mocks.getNamespaceSummary.mockResolvedValue([
    { namespace: 'character', count: 2 },
    { namespace: 'creator', count: 1 },
  ]);
  mocks.getRelations.mockResolvedValue([]);
  mocks.rename.mockResolvedValue(undefined);
  mocks.merge.mockResolvedValue(undefined);
  mocks.delete.mockResolvedValue(undefined);
  mocks.setAlias.mockResolvedValue(undefined);
  mocks.setImplication.mockResolvedValue(undefined);
  mocks.unlisten.mockResolvedValue(undefined);
  mocks.listen.mockImplementation(async (_name: string, handler: (event: { payload: { changes: Record<string, unknown> } }) => void) => {
    stateEvent = handler;
    return mocks.unlisten;
  });
  mocks.getPaginated.mockImplementation(async ({ cursor, search }: { cursor?: string | null; search?: string | null }) => {
    if (search) return { items: [nextTag], next_cursor: null };
    if (cursor) return { items: [nextTag], next_cursor: null };
    return { items: [firstTag, zeroTag], next_cursor: 'opaque-cursor' };
  });
});

async function renderScreen() {
  let result: ReturnType<typeof render>;
  await act(async () => {
    result = render(
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
    expect(screen.getByRole('navigation', { name: 'Tag groups' })).toHaveTextContent('Groups (2)');
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

  it('labels and filters the empty namespace as general rather than all tags', async () => {
    const user = setupUser();
    mocks.getNamespaceSummary.mockResolvedValue([
      { namespace: '', count: 2 },
      { namespace: 'creator', count: 1 },
    ]);
    await renderScreen();

    await user.click(screen.getByRole('button', { name: 'general 2' }));
    await settle();

    expect(within(screen.getByRole('region', { name: 'Tags' })).getByText('general')).toBeInTheDocument();
    await waitFor(() => expect(mocks.getPaginated).toHaveBeenCalledWith({
      namespace: '',
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
    await waitFor(() => expect(mocks.setAlias).toHaveBeenCalledWith('creator:bob', 'character:alice'));

    await user.click(screen.getByRole('button', { name: 'Add parent' }));
    await settle();
    const parentDialog = screen.getByRole('dialog', { name: 'Add existing parent' });
    await user.type(within(parentDialog).getByPlaceholderText('Search existing tags'), 'bob');
    await settle();
    await user.click(await within(parentDialog).findByRole('button', { name: /bob/ }));
    await settle();
    await waitFor(() => expect(mocks.setImplication).toHaveBeenCalledWith('character:alice', 'creator:bob', 'add'));
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
      stateEvent?.({ payload: { changes: { tags_changed: true } } });
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
    await waitFor(() => expect(mocks.merge).toHaveBeenCalledWith('character:alice', 'creator:bob'));
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Edit tag' })).not.toBeInTheDocument());
  });

  it('uses the shared tag context menu for every tag row', async () => {
    await renderScreen();
    const tag = await screen.findByRole('button', { name: /character:alice/ });

    fireEvent.contextMenu(tag, { clientX: 100, clientY: 100 });

    expect(await screen.findByRole('menu')).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Edit Tag' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Merge Into…' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Delete Tag' })).toBeInTheDocument();
  });
});
