import { notifyInfo } from '../lib/notify';
import { useUndoRedoStore } from '../../state/undoRedoStore';

let actionCounter = 0;

export function registerUndoAction(input: {
  label: string;
  forward: () => Promise<void> | void;
  backward: () => Promise<void> | void;
}): void {
  actionCounter += 1;
  useUndoRedoStore.getState().pushAction({
    id: `undo-${Date.now()}-${actionCounter}`,
    label: input.label,
    forward: async () => { await input.forward(); },
    backward: async () => { await input.backward(); },
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
