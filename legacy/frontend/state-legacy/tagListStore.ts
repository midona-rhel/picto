/**
 * Tag list signals — controller signals tag mutations, TagManager applies eagerly.
 *
 * Pattern matches gridMetadataStore: the controller queues specific mutations,
 * the UI subscribes and applies them locally. Backend state-change handles
 * authoritative reconciliation.
 */

import { create } from 'zustand';

export interface TagRemoval {
  tagId?: number;
  namespace?: string;
  subtag?: string;
}

export interface TagRename {
  tagId: number;
  namespace: string;
  subtag: string;
}

interface TagListState {
  pendingRemovals: TagRemoval[];
  pendingRenames: TagRename[];

  queueRemoval: (tagId: number) => void;
  queueRemovalByName: (namespace: string, subtag: string) => void;
  queueRename: (tagId: number, namespace: string, subtag: string) => void;
  drainMutations: () => { removals: TagRemoval[]; renames: TagRename[] };
}

export const useTagListStore = create<TagListState>((set, get) => ({
  pendingRemovals: [],
  pendingRenames: [],

  queueRemoval: (tagId: number) => {
    set((s) => ({ pendingRemovals: [...s.pendingRemovals, { tagId }] }));
  },

  queueRemovalByName: (namespace: string, subtag: string) => {
    set((s) => ({ pendingRemovals: [...s.pendingRemovals, { namespace, subtag }] }));
  },

  queueRename: (tagId: number, namespace: string, subtag: string) => {
    set((s) => ({ pendingRenames: [...s.pendingRenames, { tagId, namespace, subtag }] }));
  },

  drainMutations: () => {
    const { pendingRemovals, pendingRenames } = get();
    if (pendingRemovals.length > 0 || pendingRenames.length > 0) {
      set({ pendingRemovals: [], pendingRenames: [] });
    }
    return { removals: pendingRemovals, renames: pendingRenames };
  },
}));
