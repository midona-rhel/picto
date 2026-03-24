import { notifyInfo } from '../lib/notify';
import { useUndoRedoStore } from '../../state-legacy/undoRedoStore';

let actionCounter = 0;

/**
 * When true, undo registration is suppressed. Set by performUndo/performRedo
 * so that controller methods called during undo/redo execution don't create
 * recursive undo entries.
 */
let suppressRegistration = false;

/** Check if we're currently executing an undo/redo operation. */
export function isUndoRedoInProgress(): boolean {
  return suppressRegistration;
}

export function registerUndoAction(input: {
  label: string;
  forward: () => Promise<void> | void;
  backward: () => Promise<void> | void;
}): void {
  if (suppressRegistration) return;
  actionCounter += 1;
  useUndoRedoStore.getState().pushAction({
    id: `undo-${Date.now()}-${actionCounter}`,
    label: input.label,
    forward: async () => {
      suppressRegistration = true;
      try { await input.forward(); } finally { suppressRegistration = false; }
    },
    backward: async () => {
      suppressRegistration = true;
      try { await input.backward(); } finally { suppressRegistration = false; }
    },
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
