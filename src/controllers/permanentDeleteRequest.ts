/**
 * The one confirmation for permanent deletes. It says whether the delete can
 * be undone: deletes of up to the "Undo deletes of up to" setting keep their
 * files until Picto closes, larger ones remove them at once.
 */
import { getDefaultStore } from 'jotai';
import { t } from '../i18n';
import { getSettings } from '../platform/settingsApi';
import type { EntityTarget } from '../shared/types/canonical';
import { confirmModalAtom } from '../state/modals';
import { permanentlyDeleteTarget } from './entityMutations';

const DEFAULT_UNDO_LIMIT = 500;

export function permanentDeleteMessage(count: number | null, undoLimit: number): string {
  if (count != null && count <= undoLimit) {
    return t('You can undo this until Picto closes. After that the files are removed.');
  }
  if (count == null) {
    return t('Deleting up to {value0} items can be undone until Picto closes. A larger delete cannot be undone.', {
      value0: undoLimit.toLocaleString(),
    });
  }
  return t('This is more than the {value0} items Picto keeps for undo, so it cannot be undone.', {
    value0: undoLimit.toLocaleString(),
  });
}

export async function requestPermanentDelete(
  target: EntityTarget,
  count: number | null,
  options: { emptyTrash?: boolean; onError?: (error: unknown) => void } = {},
): Promise<void> {
  const undoLimit = await getSettings()
    .then((settings) => settings.undoDeleteLimit)
    .catch(() => DEFAULT_UNDO_LIMIT);
  getDefaultStore().set(confirmModalAtom, {
    open: true,
    title: options.emptyTrash
      ? t('Empty Trash')
      : count === 1 || count == null
        ? t('Delete permanently?')
        : t('Delete {value0} items permanently?', { value0: count.toLocaleString() }),
    message: permanentDeleteMessage(count, undoLimit),
    confirmLabel: options.emptyTrash ? t('Delete All') : t('Delete Permanently'),
    danger: true,
    onConfirm: () => {
      void permanentlyDeleteTarget(target).catch((error) => options.onError?.(error));
    },
  });
}
