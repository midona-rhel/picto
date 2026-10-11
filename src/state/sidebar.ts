/**
 * Sidebar state — authoritative tree + derived views.
 *
 * Jotai-owned. Controllers mutate via write atoms.
 * Runtime settle applies backend-confirmed updates.
 */

import { atom } from 'jotai';
import type { SidebarNodeDto } from '../shared/types/canonical';

const SUBSCRIPTIONS_NODE: SidebarNodeDto = {
  id: 'system:subscriptions',
  kind: 'system',
  parent_id: null,
  name: 'Subscriptions',
  icon: null,
  color: null,
  sort_order: 7,
  count: null,
  freshness: 'exact',
  selectable: true,
  expanded_by_default: false,
  meta: null,
};

// ── Authoritative ────────────────────────────────────────────────

export const sidebarNodesAtom = atom<SidebarNodeDto[]>([]);
export const sidebarEpochAtom = atom(0);
export const sidebarLoadingAtom = atom(false);
/** Lets non-sidebar commands hand a newly created node to the tree's rename owner. */
export const pendingSidebarRenameNodeIdAtom = atom<string | null>(null);
/** Newly created tree node that must be made visible even when its ancestors are collapsed. */
export const pendingSidebarRevealNodeIdAtom = atom<string | null>(null);

// ── Derived: node kind filters ───────────────────────────────────

export const systemNodesAtom = atom((get) =>
  withSubscriptionsNode(
    get(sidebarNodesAtom).filter((n) => n.kind === 'system' && n.id !== 'system:all'),
  ),
);

export const folderNodesAtom = atom((get) =>
  get(sidebarNodesAtom).filter((n) => n.kind === 'folder'),
);

export const smartFolderNodesAtom = atom((get) =>
  get(sidebarNodesAtom).filter((n) => n.kind === 'smart_folder'),
);

function withSubscriptionsNode(nodes: SidebarNodeDto[]): SidebarNodeDto[] {
  if (nodes.some((node) => node.id === SUBSCRIPTIONS_NODE.id)) return nodes;
  const next = [...nodes];
  const duplicatesIndex = next.findIndex((node) => node.id === 'system:duplicates');
  if (duplicatesIndex >= 0) {
    next.splice(duplicatesIndex + 1, 0, SUBSCRIPTIONS_NODE);
    return next;
  }
  next.push(SUBSCRIPTIONS_NODE);
  return next;
}

// ── Write atoms (used by controllers) ────────────────────────────

export const setSidebarTreeAtom = atom(
  null,
  (_get, set, tree: { nodes: SidebarNodeDto[]; epoch: number }) => {
    set(sidebarNodesAtom, tree.nodes);
    set(sidebarEpochAtom, tree.epoch);
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

export const patchSmartFolderNodeAtom = atom(
  null,
  (get, set, { smartFolderId, patch }: { smartFolderId: number; patch: Partial<SidebarNodeDto> }) => {
    const sid = `smart:${smartFolderId}`;
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => (n.id === sid ? { ...n, ...patch } : n)),
    );
  },
);

// ── Batch delta atoms (used by runtime settle) ───────────────────

