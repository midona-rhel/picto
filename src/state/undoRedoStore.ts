import { create } from 'zustand';

export interface UndoRedoAction {
  id: string;
  label: string;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
}

interface UndoRedoState {
  undoStack: UndoRedoAction[];
  redoStack: UndoRedoAction[];
  inFlight: boolean;
  lastError: string | null;
  pushAction: (action: UndoRedoAction) => void;
  undo: () => Promise<UndoRedoAction | null>;
  redo: () => Promise<UndoRedoAction | null>;
  clear: () => void;
}

const MAX_STACK_SIZE = 100;

export const useUndoRedoStore = create<UndoRedoState>((set, get) => ({
  undoStack: [],
  redoStack: [],
  inFlight: false,
  lastError: null,

  pushAction: (action) => {
    set((state) => {
      const nextUndo = [...state.undoStack, action];
      if (nextUndo.length > MAX_STACK_SIZE) nextUndo.shift();
      return {
        undoStack: nextUndo,
        redoStack: [],
        lastError: null,
      };
    });
  },

  undo: async () => {
    const { undoStack, inFlight } = get();
    if (inFlight || undoStack.length === 0) return null;
    const action = undoStack[undoStack.length - 1];
    set({ inFlight: true, lastError: null });
    try {
      await action.undo();
      set((state) => ({
        undoStack: state.undoStack.slice(0, -1),
        redoStack: [...state.redoStack, action],
        inFlight: false,
        lastError: null,
      }));
      return action;
    } catch (err) {
      set({ inFlight: false, lastError: String(err) });
      throw err;
    }
  },

  redo: async () => {
    const { redoStack, inFlight } = get();
    if (inFlight || redoStack.length === 0) return null;
    const action = redoStack[redoStack.length - 1];
    set({ inFlight: true, lastError: null });
    try {
      await action.redo();
      set((state) => ({
        redoStack: state.redoStack.slice(0, -1),
        undoStack: [...state.undoStack, action],
        inFlight: false,
        lastError: null,
      }));
      return action;
    } catch (err) {
      set({ inFlight: false, lastError: String(err) });
      throw err;
    }
  },

  clear: () => {
    set({
      undoStack: [],
      redoStack: [],
      inFlight: false,
      lastError: null,
    });
  },
}));

