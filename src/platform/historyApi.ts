import { invoke } from './ipc';
import type { MutationReceipt } from '../shared/types/generated/application/MutationReceipt';

export interface HistoryEntrySummary {
  entry_id: number;
  command: string;
  /** Developer wording; shown only for a command `describeHistoryEntry` does not know. */
  label: string;
  /** Items the change touched, or 0 when it is not about items. */
  count: number;
  /** What it set: a status (`trash`, `inbox`, `active`), stars, or `add`/`remove`. */
  detail: string | null;
}

export interface HistoryState {
  undo: HistoryEntrySummary | null;
  redo: HistoryEntrySummary | null;
}

export interface HistoryOperationResult {
  entry: HistoryEntrySummary;
  state: HistoryState;
  receipt: MutationReceipt;
}

export function getHistoryState(): Promise<HistoryState> {
  return invoke<HistoryState>('history.state');
}

export function undoHistory(): Promise<HistoryOperationResult> {
  return invoke<HistoryOperationResult>('history.undo');
}

export function redoHistory(): Promise<HistoryOperationResult> {
  return invoke<HistoryOperationResult>('history.redo');
}
