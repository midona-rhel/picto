import { create } from 'zustand';

import type { ManualImportProgressEvent } from '../shared/types/api/events';

type ManualImportStatus = 'idle' | 'running' | 'completed';
type ManualImportSource = 'none' | 'renderer' | 'backend';

interface ManualImportState {
  visible: boolean;
  status: ManualImportStatus;
  source: ManualImportSource;
  label: string;
  done: number;
  total: number;
  imported: number;
  skipped: number;
  errors: number;
  start: (total: number, label?: string, source?: Exclude<ManualImportSource, 'none'>) => void;
  startBackend: (label?: string) => void;
  update: (progress: ManualImportProgressEvent) => void;
  setProgress: (progress: { done: number; total: number; imported: number; skipped: number; errors: number; label?: string }) => void;
  finish: (result: { imported: number; skipped: number; errors: number }) => void;
  fail: () => void;
  clear: () => void;
}

let hideTimer: number | null = null;

function clearHideTimer(): void {
  if (hideTimer != null) {
    window.clearTimeout(hideTimer);
    hideTimer = null;
  }
}

export const useManualImportStore = create<ManualImportState>((set, get) => ({
  visible: false,
  status: 'idle',
  source: 'none',
  label: 'Adding files',
  done: 0,
  total: 0,
  imported: 0,
  skipped: 0,
  errors: 0,

  start: (total, label = 'Adding files', source = 'renderer') => {
    clearHideTimer();
    set({
      visible: true,
      status: 'running',
      source,
      label,
      done: 0,
      total,
      imported: 0,
      skipped: 0,
      errors: 0,
    });
  },

  startBackend: (label = 'Adding files') => {
    get().start(0, label, 'backend');
  },

  update: (progress) => {
    const state = get();
    if (state.status === 'idle') {
      get().start(progress.total, undefined, 'backend');
    } else if (state.source !== 'backend') {
      return;
    }
    set({
      visible: true,
      status: 'running',
      source: 'backend',
      done: progress.done,
      total: progress.total,
      imported: progress.imported,
      skipped: progress.skipped,
      errors: progress.errors,
    });
  },

  setProgress: (progress) => {
    if (get().status === 'idle') {
      get().start(progress.total, progress.label);
    }
    set((state) => ({
      visible: true,
      status: 'running',
      source: 'renderer',
      label: progress.label ?? state.label,
      done: progress.done,
      total: progress.total,
      imported: progress.imported,
      skipped: progress.skipped,
      errors: progress.errors,
    }));
  },

  finish: ({ imported, skipped, errors }) => {
    clearHideTimer();
    const total = get().total;
    set({
      visible: true,
      status: 'completed',
      source: 'none',
      done: total,
      total,
      imported,
      skipped,
      errors,
    });
    hideTimer = window.setTimeout(() => {
      get().clear();
    }, 1400);
  },

  fail: () => {
    clearHideTimer();
    set({
      visible: false,
      status: 'idle',
      source: 'none',
      done: 0,
      total: 0,
      imported: 0,
      skipped: 0,
      errors: 0,
    });
  },

  clear: () => {
    clearHideTimer();
    set({
      visible: false,
      status: 'idle',
      source: 'none',
      done: 0,
      total: 0,
      imported: 0,
      skipped: 0,
      errors: 0,
    });
  },
}));
