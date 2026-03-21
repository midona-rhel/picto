import { notifyInfo } from '../lib/notify';
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
  const action = await store.undo().catch(() => null);
  if (!action) {
    notifyInfo('Nothing to undo', 'Undo');
    return false;
  }
  notifyInfo(action.label, 'Undo');
  return true;
}

export async function performRedo(): Promise<boolean> {
  const store = useUndoRedoStore.getState();
  const action = await store.redo().catch(() => null);
  if (!action) {
    notifyInfo('Nothing to redo', 'Redo');
    return false;
  }
  notifyInfo(action.label, 'Redo');
  return true;
}
