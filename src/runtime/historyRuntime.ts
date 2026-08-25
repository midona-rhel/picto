import { isEditableTarget } from '../app/editableTarget';
import { listen } from '../platform/ipc';
import {
  getHistoryState,
  redoHistory,
  undoHistory,
  type HistoryOperationResult,
} from '../platform/historyApi';
import {
  showErrorNotification,
  showInfoNotification,
  showSuccessNotification,
} from '../shared/lib/notifications';

let operationPending = false;

function runNativeEditHistory(direction: 'undo' | 'redo'): boolean {
  const active = document.activeElement;
  if (!isEditableTarget(active)) return false;
  document.execCommand(direction);
  return true;
}

async function perform(direction: 'undo' | 'redo'): Promise<void> {
  if (operationPending || runNativeEditHistory(direction)) return;
  operationPending = true;
  try {
    const result = direction === 'undo' ? await undoHistory() : await redoHistory();
    showHistoryResult(direction, result);
  } catch (error) {
    showErrorNotification({
      title: direction === 'undo' ? 'Could not undo' : 'Could not redo',
      message: error instanceof Error ? error.message : String(error),
    });
  } finally {
    operationPending = false;
  }
}

function showHistoryResult(direction: 'undo' | 'redo', result: HistoryOperationResult): void {
  const reverse = direction === 'undo' ? result.state.redo : result.state.undo;
  showInfoNotification({
    title: `${direction === 'undo' ? 'Undid' : 'Redid'} ${result.entry.label}`,
    message: '',
    action: reverse ? {
      label: direction === 'undo' ? 'Redo' : 'Undo',
      onClick: () => { void perform(direction === 'undo' ? 'redo' : 'undo'); },
    } : undefined,
  });
}

export async function announceUndoableMutation(command: string): Promise<void> {
  try {
    const state = await getHistoryState();
    if (state.undo?.command !== command) return;
    showSuccessNotification({
      title: state.undo.label,
      message: '',
      action: {
        label: 'Undo',
        onClick: () => { void perform('undo'); },
      },
    });
  } catch {
    // The mutation already committed. Notification availability must never
    // turn a successful write into a user-visible operation failure.
  }
}

export function startHistoryRuntime(): () => void {
  let disposed = false;
  const disposers: Array<() => void> = [];
  const subscribe = (name: 'menu:undo' | 'menu:redo', direction: 'undo' | 'redo') => {
    void listen(name, () => { void perform(direction); }).then((dispose) => {
      if (disposed) dispose();
      else disposers.push(dispose);
    }).catch((error) => {
      console.error(`Failed to subscribe to ${name}`, error);
    });
  };
  subscribe('menu:undo', 'undo');
  subscribe('menu:redo', 'redo');
  return () => {
    disposed = true;
    for (const dispose of disposers) dispose();
  };
}

export function resetHistoryRuntimeForTests(): void {
  operationPending = false;
}
