/** Permanent-delete confirmation, including a fresh warning if undo eligibility changes. */
import { getDefaultStore } from 'jotai';
import { t } from '../i18n';
import { getSettings } from '../platform/settingsApi';
import { getSelectionSummary } from '../platform/entityApi';
import type { EntityTarget } from '../shared/types/canonical';
import { confirmModalAtom } from '../state/modals';
import { permanentlyDeleteTarget } from './entityMutations';
import { showErrorNotification } from '../shared/lib/notifications';

export function permanentDeleteMessage(count: number | null, undoLimit: number): string {
  if (undoLimit === 0) return t('Delete undo is disabled. This cannot be undone.');
  if (count != null && count <= undoLimit) {
    return t('You can undo this until Picto closes. After that the files are removed.');
  }
  if (count == null) {
    return t('Deleting up to {value0} media items, including collection members, can be undone until Picto closes. Larger deletes cannot be undone.', {
      value0: undoLimit.toLocaleString(),
    });
  }
  return t('This exceeds the undo limit of {value0} media items, including collection members, so it cannot be undone.', {
    value0: undoLimit.toLocaleString(),
  });
}

export async function requestPermanentDelete(
  target: EntityTarget,
  _count: number | null,
  options: {
    emptyTrash?: boolean;
    onError?: (error: unknown) => void;
    onlyNonUndoable?: boolean;
    remove?: (target: EntityTarget, requireUndoable: boolean) => Promise<void>;
  } = {},
): Promise<boolean> {
  try {
    const undoLimit = await getSettings()
      .then((settings) => settings.undoDeleteLimit)
      .catch(() => 0);
    const summary = await getSelectionSummary(target);
    const count = summary.selected_count;
    const remove = options.remove ?? permanentlyDeleteTarget;
    const requireUndoable = undoLimit > 0 && summary.media_count <= undoLimit;
    const confirm = (message: string) => new Promise<boolean>((resolve) => {
      getDefaultStore().set(confirmModalAtom, {
        open: true,
        title: options.emptyTrash
          ? t('Empty Trash')
          : count === 1
            ? t('Delete permanently?')
            : t('Delete {value0} items permanently?', { value0: count.toLocaleString() }),
        message,
        confirmLabel: options.emptyTrash ? t('Delete All') : t('Delete Permanently'),
        danger: true,
        onConfirm: () => resolve(true),
        onCancel: () => resolve(false),
      });
    });
    if ((!options.onlyNonUndoable || !requireUndoable)
      && !await confirm(permanentDeleteMessage(summary.media_count, undoLimit))) return false;

    try {
      await remove(target, requireUndoable);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (!requireUndoable || !message.includes('operation requires non-undoable confirmation')) throw error;
      // The writer resolved a different selection or limit. Never silently
      // downgrade the promise made by the first confirmation.
      if (!await confirm(t('This delete can no longer be undone. Delete permanently anyway?'))) return false;
      await remove(target, false);
    }
    return true;
  } catch (error) {
    if (options.onError) options.onError(error);
    else showErrorNotification({ title: t('Delete Permanently'), message: error instanceof Error ? error.message : String(error) });
    return false;
  }
}
