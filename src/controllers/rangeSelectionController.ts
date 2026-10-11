import { getDefaultStore } from 'jotai';
import { t } from '../i18n';
import { getRootPositions } from '../platform/entityApi';
import { showWarningNotification } from '../shared/lib/notifications';
import type { EntityViewQuery } from '../shared/types/canonical';
import { rangeRootIds, type RangeSpan, type RootPositions } from '../state/rangeSelection';
import { gridSelectionActionAtom, gridSelectionAtom } from '../state/selection';

const store = getDefaultStore();
let reconciliationGeneration = 0;

async function positionsOf(query: EntityViewQuery, ids: number[]): Promise<RootPositions> {
  if (ids.length === 0) return new Map();
  const positions = await getRootPositions(query, ids);
  return new Map(ids.map((id, index) => [id, positions[index] ?? null]));
}

/**
 * Cmd/Ctrl+Shift-click: add `span` to the selection. The roots selected one
 * by one and those toggled off may be anywhere in the view, so their current
 * positions come from the backend before the span is merged in.
 */
export async function addSelectionSpan(query: EntityViewQuery, span: RangeSpan): Promise<void> {
  const selection = store.get(gridSelectionAtom);
  const ids = new Set<number>(selection.excludedItemIds);
  (selection.range ? selection.range.includedItemIds : selection.itemIds).forEach((id) => ids.add(id));
  const positions = await positionsOf(query, [...ids]);
  if (store.get(gridSelectionAtom) !== selection) return;
  store.set(gridSelectionActionAtom, { type: 'range_add', query, span, positions });
}

/**
 * After the view changes, move a range selection's ends, additions and
 * exclusions to where the backend now has them, so highlights and the count
 * match what an operation on the selection would change.
 */
export async function reconcileSelectionRange(): Promise<void> {
  const generation = ++reconciliationGeneration;
  const selection = store.get(gridSelectionAtom);
  if (!selection.range || !selection.query) return;
  const ids = new Set(rangeRootIds(selection.range, selection.excludedItemIds));
  // Merging spans can leave the next Shift-click anchor inside a span.
  if (selection.anchor?.kind === 'item') ids.add(selection.anchor.id);
  const positions = await positionsOf(selection.query, [...ids]);
  if (generation !== reconciliationGeneration || store.get(gridSelectionAtom) !== selection) return;
  const lostEnd = selection.range.spans.some((span) => positions.get(span.anchor.id) === null
    || positions.get(span.focus.id) === null);
  store.set(gridSelectionActionAtom, { type: 'range_reconcile', positions });
  if (lostEnd) {
    showWarningNotification({
      title: t('Part of the selection left this view'),
      message: t('A selected range lost its first or last item, so that range is no longer selected.'),
    });
  }
}
