/**
 * Sidebar state — authoritative tree + derived views.
 *
 * Jotai-owned. Controllers mutate via write atoms.
 * Runtime settle applies backend-confirmed updates.
 */

import { atom } from 'jotai';
import type { SidebarNodeDto } from '../shared/types/canonical';

// ── Authoritative ────────────────────────────────────────────────

export const sidebarNodesAtom = atom<SidebarNodeDto[]>([]);
export const sidebarEpochAtom = atom(0);
export const sidebarLoadingAtom = atom(false);

// ── Derived: node kind filters ───────────────────────────────────

export const systemNodesAtom = atom((get) =>
  get(sidebarNodesAtom).filter((n) => n.kind === 'system' && n.id !== 'system:all'),
);

export const folderNodesAtom = atom((get) =>
  get(sidebarNodesAtom).filter((n) => n.kind === 'folder'),
);

export const smartFolderNodesAtom = atom((get) =>
  get(sidebarNodesAtom).filter((n) => n.kind === 'smart_folder'),
);

// ── Derived: scope counts ────────────────────────────────────────

export const scopeCountsAtom = atom((get) => {
  const nodes = get(sidebarNodesAtom);
  const find = (id: string) => nodes.find((n) => n.id === id)?.count ?? 0;
  return {
    active: find('system:active'),
    inbox: find('system:inbox'),
    trash: find('system:trash'),
    uncategorized: find('system:uncategorized'),
    untagged: find('system:untagged'),
    duplicates: find('system:duplicates'),
  };
});

// ── Write atoms (used by controllers) ────────────────────────────

export const setSidebarTreeAtom = atom(
  null,
  (_get, set, tree: { nodes: SidebarNodeDto[]; epoch: number }) => {
    set(sidebarNodesAtom, tree.nodes);
    set(sidebarEpochAtom, tree.epoch);
  },
);

export const applySidebarCountsAtom = atom(
  null,
  (get, set, counts: { active: number; inbox: number; trash: number }) => {
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => {
        if (n.id === 'system:active') return { ...n, count: counts.active };
        if (n.id === 'system:inbox') return { ...n, count: counts.inbox };
        if (n.id === 'system:trash') return { ...n, count: counts.trash };
        return n;
      }),
    );
  },
);

export const adjustFolderCountAtom = atom(
  null,
  (get, set, { folderId, delta }: { folderId: number; delta: number }) => {
    const fid = `folder:${folderId}`;
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) =>
        n.id === fid ? { ...n, count: (n.count ?? 0) + delta } : n,
      ),
    );
  },
);

export const insertFolderNodeAtom = atom(
  null,
  (get, set, node: SidebarNodeDto) => {
    set(sidebarNodesAtom, [...get(sidebarNodesAtom), node]);
  },
);

export const removeFolderNodeAtom = atom(
  null,
  (get, set, folderId: number) => {
    const fid = `folder:${folderId}`;
    set(sidebarNodesAtom, get(sidebarNodesAtom).filter((n) => n.id !== fid));
  },
);

export const patchFolderNodeAtom = atom(
  null,
  (get, set, { folderId, patch }: { folderId: number; patch: Partial<SidebarNodeDto> }) => {
    const fid = `folder:${folderId}`;
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => (n.id === fid ? { ...n, ...patch } : n)),
    );
  },
);

// ── Smart folder write atoms ─────────────────────────────────────

export const removeSmartFolderNodeAtom = atom(
  null,
  (get, set, sfId: number) => {
    const sid = `smart:${sfId}`;
    set(sidebarNodesAtom, get(sidebarNodesAtom).filter((n) => n.id !== sid));
  },
);

export const patchSmartFolderNodeAtom = atom(
  null,
  (get, set, { sfId, patch }: { sfId: number; patch: Partial<SidebarNodeDto> }) => {
    const sid = `smart:${sfId}`;
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => (n.id === sid ? { ...n, ...patch } : n)),
    );
  },
);
