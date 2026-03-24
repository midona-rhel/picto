import { create } from 'zustand';

export interface LogEntry {
  id: number;
  level: 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';
  target: string;
  message: string;
  timestamp: string;
}

const MAX_ENTRIES = 1000;
let nextId = 1;

interface LogState {
  entries: LogEntry[];
  addEntry: (raw: Omit<LogEntry, 'id'>) => void;
  clear: () => void;
}

export const useLogStore = create<LogState>((set) => ({
  entries: [],
  addEntry: (raw) => set((state) => {
    const entry: LogEntry = { ...raw, id: nextId++ };
    const next = [...state.entries, entry];
    if (next.length > MAX_ENTRIES) {
      next.splice(0, next.length - MAX_ENTRIES);
    }
    return { entries: next };
  }),
  clear: () => set({ entries: [] }),
}));
