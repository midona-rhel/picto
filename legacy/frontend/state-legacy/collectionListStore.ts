/**
 * Collection list signals — controller signals collection mutations,
 * Collections.tsx applies eagerly.
 *
 * Same pattern as tagListStore: controller queues specific mutations,
 * UI subscribes and applies locally. Backend state-change handles
 * authoritative reconciliation.
 */

import { create } from 'zustand';

export interface CollectionRemoval {
  id: number;
}

export interface CollectionUpdate {
  id: number;
  name?: string;
}

interface CollectionListState {
  pendingRemovals: CollectionRemoval[];
  pendingUpdates: CollectionUpdate[];
  pendingCreatedId: number | null;

  queueRemoval: (id: number) => void;
  queueUpdate: (id: number, name?: string) => void;
  queueCreated: (id: number) => void;
  drainMutations: () => {
    removals: CollectionRemoval[];
    updates: CollectionUpdate[];
    createdId: number | null;
  };
}

export const useCollectionListStore = create<CollectionListState>((set, get) => ({
  pendingRemovals: [],
  pendingUpdates: [],
  pendingCreatedId: null,

  queueRemoval: (id: number) => {
    set((s) => ({ pendingRemovals: [...s.pendingRemovals, { id }] }));
  },

  queueUpdate: (id: number, name?: string) => {
    set((s) => ({ pendingUpdates: [...s.pendingUpdates, { id, name }] }));
  },

  queueCreated: (id: number) => {
    set({ pendingCreatedId: id });
  },

  drainMutations: () => {
    const { pendingRemovals, pendingUpdates, pendingCreatedId } = get();
    if (pendingRemovals.length > 0 || pendingUpdates.length > 0 || pendingCreatedId != null) {
      set({ pendingRemovals: [], pendingUpdates: [], pendingCreatedId: null });
    }
    return { removals: pendingRemovals, updates: pendingUpdates, createdId: pendingCreatedId };
  },
}));
