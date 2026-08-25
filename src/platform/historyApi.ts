import { invoke } from './ipc';
import type { MutationReceipt } from '../shared/types/generated/application/MutationReceipt';

export interface HistoryEntrySummary {
  entry_id: number;
  command: string;
  label: string;
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
