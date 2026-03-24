import { create } from 'zustand';

export interface UndoableAction {
  id: string;
  label: string;
  /** The action that was performed (called on redo). */
  forward: () => Promise<void>;
  /** The inverse of the action (called on undo). */
  backward: () => Promise<void>;
}

/** @deprecated — use UndoableAction instead. Kept for migration compatibility. */
export type UndoRedoAction = UndoableAction;

interface UndoRedoState {
  undoStack: UndoableAction[];
  redoStack: UndoableAction[];
  inFlight: boolean;
  lastError: string | null;
  pushAction: (action: UndoableAction) => void;
  undo: () => Promise<UndoableAction | null>;
  redo: () => Promise<UndoableAction | null>;
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
      await action.backward();
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
      await action.forward();
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
