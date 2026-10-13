import { describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { permanentDeleteMessage, requestPermanentDelete } from './permanentDeleteRequest';
import { getSelectionSummary } from '../platform/entityApi';
import { getSettings } from '../platform/settingsApi';
import { confirmModalAtom } from '../state/modals';
import type { SelectionSummary } from '../shared/types/canonical';

vi.mock('../platform/entityApi', () => ({ getSelectionSummary: vi.fn() }));
vi.mock('../platform/settingsApi', () => ({ getSettings: vi.fn() }));

describe('permanentDeleteMessage', () => {
  it('says a delete within the limit can be undone until Picto closes', () => {
    expect(permanentDeleteMessage(3, 500)).toBe('You can undo this until Picto closes. After that the files are removed.');
  });

  it('warns that a delete past the limit cannot be undone', () => {
    expect(permanentDeleteMessage(501, 500)).toBe('This is more than the 500 items Picto keeps for undo, so it cannot be undone.');
  });

  it('states the limit when the count is not known', () => {
    expect(permanentDeleteMessage(null, 500)).toContain('500');
  });

  it('warns when undo is disabled', () => {
    expect(permanentDeleteMessage(1, 0)).toContain('cannot be undone');
  });

  it('confirms using resolved media count rather than the caller root count', async () => {
    vi.mocked(getSettings).mockResolvedValue({ undoDeleteLimit: 2 } as Awaited<ReturnType<typeof getSettings>>);
    vi.mocked(getSelectionSummary).mockResolvedValue({ selected_count: 1, media_count: 3 } as SelectionSummary);
    const remove = vi.fn().mockResolvedValue(undefined);
    const target = { kind: 'explicit' as const, root_ids: [4] };
    await requestPermanentDelete(target, 1, { remove });
    const confirmation = getDefaultStore().get(confirmModalAtom);
    expect(confirmation.message).toContain('cannot be undone');
    expect(remove).not.toHaveBeenCalled();
    confirmation.onConfirm();
    expect(remove).toHaveBeenCalledWith(target);
  });
});
