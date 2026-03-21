import { notifyError, notifyInfo } from '../lib/notify';
import { useUndoRedoStore, type UndoRedoAction } from '../../state/undoRedoStore';

let actionCounter = 0;

export function registerUndoAction(input: Omit<UndoRedoAction, 'id'>): void {
  actionCounter += 1;
  useUndoRedoStore.getState().pushAction({
    id: `undo-${Date.now()}-${actionCounter}`,
    ...input,
  });
}

export async function performUndo(): Promise<boolean> {
  const store = useUndoRedoStore.getState();
  // Try up to 5 actions in case some fail (e.g. file deleted since action was recorded)
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      const action = await store.undo();
      if (!action) {
        if (attempt === 0) notifyInfo('Nothing to undo', 'Undo');
        return attempt > 0;
      }
      notifyInfo(action.label, 'Undo');
      return true;
    } catch {
      // Action failed (likely missing file) — skip and try the next one
      continue;
    }
  }
  notifyInfo('Nothing to undo', 'Undo');
  return false;
}

export async function performRedo(): Promise<boolean> {
  const store = useUndoRedoStore.getState();
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      const action = await store.redo();
      if (!action) {
        if (attempt === 0) notifyInfo('Nothing to redo', 'Redo');
        return attempt > 0;
      }
      notifyInfo(action.label, 'Redo');
      return true;
    } catch {
      continue;
    }
  }
  notifyInfo('Nothing to redo', 'Redo');
  return false;
}
