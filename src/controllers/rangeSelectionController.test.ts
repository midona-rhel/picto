import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getRootPositions } from '../platform/entityApi';
import { showWarningNotification } from '../shared/lib/notifications';
import { currentGridQueryAtom } from '../state/grid';
import { gridSelectionActionAtom, selectionCountAtom, selectionTargetAtom } from '../state/selection';
import { addSelectionSpan, reconcileSelectionRange } from './rangeSelectionController';

vi.mock('../platform/entityApi', () => ({ getRootPositions: vi.fn() }));
vi.mock('../shared/lib/notifications', () => ({ showWarningNotification: vi.fn() }));

const store = getDefaultStore();

describe('range selection controller', () => {
  beforeEach(() => {
    store.set(gridSelectionActionAtom, { type: 'clear' });
    vi.mocked(getRootPositions).mockReset();
    vi.mocked(showWarningNotification).mockClear();
  });

  it('does not restore a range after the user clears selection during the lookup', async () => {
    const query = store.get(currentGridQueryAtom);
    store.set(gridSelectionActionAtom, { type: 'replace_items', itemIds: new Set([7]), anchor: 7, anchorIndex: 0 });
    let finish!: (positions: Array<number | null>) => void;
    vi.mocked(getRootPositions).mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
    const pending = addSelectionSpan(query, { anchor: { id: 7, index: 0 }, focus: { id: 9, index: 199 } });
    store.set(gridSelectionActionAtom, { type: 'clear' });
    finish([0]);
    await pending;
    expect(store.get(selectionCountAtom)).toBe(0);
  });

  it('settles the latest refresh when two position lookups overlap', async () => {
    const query = store.get(currentGridQueryAtom);
    store.set(gridSelectionActionAtom, { type: 'range_query', query, anchor: { id: 1, index: 0 }, focus: { id: 5, index: 9 } });
    let first!: (positions: Array<number | null>) => void;
    let second!: (positions: Array<number | null>) => void;
    vi.mocked(getRootPositions)
      .mockReturnValueOnce(new Promise((resolve) => { first = resolve; }))
      .mockReturnValueOnce(new Promise((resolve) => { second = resolve; }));
    const older = reconcileSelectionRange();
    const newer = reconcileSelectionRange();
    first([0, 5]);
    await older;
    second([3, 4]);
    await newer;
    expect(store.get(selectionCountAtom)).toBe(2);
  });

  it('asks the backend where the earlier selection sits before adding a span', async () => {
    const query = store.get(currentGridQueryAtom);
    store.set(gridSelectionActionAtom, { type: 'replace_items', itemIds: new Set([7, 8]), anchor: 7, anchorIndex: 0 });
    vi.mocked(getRootPositions).mockResolvedValueOnce([0, 150]);
    await addSelectionSpan(query, { anchor: { id: 7, index: 100 }, focus: { id: 9, index: 199 } });
    expect(getRootPositions).toHaveBeenCalledWith(query, [7, 8]);
    // Root 8 sits inside the new span, so it is not counted twice.
    expect(store.get(selectionCountAtom)).toBe(101);
    expect(store.get(selectionTargetAtom)).toMatchObject({ included_root_ids: [7] });
  });

  it('moves a range to the backend positions and warns when an end left the view', async () => {
    const query = store.get(currentGridQueryAtom);
    store.set(gridSelectionActionAtom, { type: 'range_query', query, anchor: { id: 1, index: 0 }, focus: { id: 5, index: 9 } });
    vi.mocked(getRootPositions).mockResolvedValueOnce([3, 4]);
    await reconcileSelectionRange();
    expect(store.get(selectionCountAtom)).toBe(2);
    expect(showWarningNotification).not.toHaveBeenCalled();

    vi.mocked(getRootPositions).mockResolvedValueOnce([3, null]);
    await reconcileSelectionRange();
    expect(store.get(selectionCountAtom)).toBe(0);
    expect(showWarningNotification).toHaveBeenCalledOnce();
  });
});
