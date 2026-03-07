import { notifyError, notifyInfo } from '../lib/notify';
import { useUndoRedoStore, type UndoRedoAction } from '../../state/undoRedoStore';

let actionCounter = 0;

export function deepClone<T>(value: T): T {
  if (typeof structuredClone === 'function') return structuredClone(value);
  return JSON.parse(JSON.stringify(value)) as T;
}

export function registerUndoAction(input: Omit<UndoRedoAction, 'id'>): void {
  actionCounter += 1;
  useUndoRedoStore.getState().pushAction({
    id: `undo-${Date.now()}-${actionCounter}`,
    ...input,
  });
}

export async function performUndo(): Promise<boolean> {
  try {
    const action = await useUndoRedoStore.getState().undo();
    if (!action) {
      notifyInfo('Nothing to undo', 'Undo');
      return false;
    }
    notifyInfo(action.label, 'Undo');
    return true;
  } catch (err) {
    notifyError(err, 'Undo Failed');
    return false;
  }
}

export async function performRedo(): Promise<boolean> {
  try {
    const action = await useUndoRedoStore.getState().redo();
    if (!action) {
      notifyInfo('Nothing to redo', 'Redo');
      return false;
    }
    notifyInfo(action.label, 'Redo');
    return true;
  } catch (err) {
    notifyError(err, 'Redo Failed');
    return false;
  }
}
