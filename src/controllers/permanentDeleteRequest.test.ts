import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { permanentDeleteMessage, requestPermanentDelete } from './permanentDeleteRequest';
import { getSelectionSummary } from '../platform/entityApi';
import { getSettings } from '../platform/settingsApi';
import { confirmModalAtom } from '../state/modals';
import type { SelectionSummary } from '../shared/types/canonical';

vi.mock('../platform/entityApi', () => ({ getSelectionSummary: vi.fn() }));
vi.mock('../platform/settingsApi', () => ({ getSettings: vi.fn() }));

describe('permanentDeleteMessage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getDefaultStore().set(confirmModalAtom, { open: false, title: '', message: '', onConfirm: () => {} });
  });
  it('says a delete within the limit can be undone until Picto closes', () => {
    expect(permanentDeleteMessage(3, 500)).toBe('You can undo this until Picto closes. After that the files are removed.');
  });

  it('warns that a delete past the limit cannot be undone', () => {
    expect(permanentDeleteMessage(501, 500)).toBe('This exceeds the undo limit of 500 media items, including collection members, so it cannot be undone.');
  });

  it('states the limit when the count is not known', () => {
    expect(permanentDeleteMessage(null, 500)).toContain('500 media items, including collection members');
  });

  it('warns when undo is disabled', () => {
    expect(permanentDeleteMessage(1, 0)).toBe('Delete undo is disabled. This cannot be undone.');
    expect(permanentDeleteMessage(null, 0)).toBe('Delete undo is disabled. This cannot be undone.');
  });

  it('confirms using resolved media count rather than the caller root count', async () => {
    vi.mocked(getSettings).mockResolvedValue({ undoDeleteLimit: 2 } as Awaited<ReturnType<typeof getSettings>>);
    vi.mocked(getSelectionSummary).mockResolvedValue({ selected_count: 1, media_count: 3 } as SelectionSummary);
    const remove = vi.fn().mockResolvedValue(undefined);
    const target = { kind: 'explicit' as const, root_ids: [4] };
    const result = requestPermanentDelete(target, 1, { remove });
    await vi.waitFor(() => expect(getDefaultStore().get(confirmModalAtom).open).toBe(true));
    const confirmation = getDefaultStore().get(confirmModalAtom);
    expect(confirmation.message).toContain('cannot be undone');
    expect(remove).not.toHaveBeenCalled();
    confirmation.onConfirm();
    await expect(result).resolves.toBe(true);
    expect(remove).toHaveBeenCalledWith(target, false);
  });

  it.each([false, true])('requires fresh consent after selection growth (confirm=%s)', async (confirmRetry) => {
    vi.mocked(getSettings).mockResolvedValue({ undoDeleteLimit: 2 } as Awaited<ReturnType<typeof getSettings>>);
    vi.mocked(getSelectionSummary).mockResolvedValue({ selected_count: 1, media_count: 1 } as SelectionSummary);
    const remove = vi.fn().mockRejectedValueOnce(new Error('operation requires non-undoable confirmation')).mockResolvedValue(undefined);
    const target = { kind: 'explicit' as const, root_ids: [4] };
    const result = requestPermanentDelete(target, 1, { remove, onlyNonUndoable: true });
    await vi.waitFor(() => expect(getDefaultStore().get(confirmModalAtom).message).toContain('can no longer be undone'));
    expect(remove).toHaveBeenCalledTimes(1);
    expect(remove).toHaveBeenCalledWith(target, true);
    const confirmation = getDefaultStore().get(confirmModalAtom);
    if (confirmRetry) confirmation.onConfirm();
    else confirmation.onCancel?.();
    await expect(result).resolves.toBe(confirmRetry);
    expect(remove).toHaveBeenCalledTimes(confirmRetry ? 2 : 1);
    if (confirmRetry) expect(remove).toHaveBeenLastCalledWith(target, false);
  });

  it('does not report success before the confirmed mutation finishes, or after failure', async () => {
    vi.mocked(getSettings).mockResolvedValue({ undoDeleteLimit: 0 } as Awaited<ReturnType<typeof getSettings>>);
    vi.mocked(getSelectionSummary).mockResolvedValue({ selected_count: 1, media_count: 1 } as SelectionSummary);
    let fail!: (error: Error) => void;
    const remove = vi.fn(() => new Promise<void>((_, reject) => { fail = reject; }));
    const onError = vi.fn();
    let settled = false;
    const result = requestPermanentDelete({ kind: 'explicit', root_ids: [4] }, 1, { remove, onError })
      .then((value) => { settled = true; return value; });
    await vi.waitFor(() => expect(getDefaultStore().get(confirmModalAtom).message).toContain('disabled'));
    getDefaultStore().get(confirmModalAtom).onConfirm();
    await vi.waitFor(() => expect(remove).toHaveBeenCalled());
    expect(settled).toBe(false);
    fail(new Error('delete failed'));
    await expect(result).resolves.toBe(false);
    expect(onError).toHaveBeenCalledWith(new Error('delete failed'));
  });
});
