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

/** Apply sidebar system scope counts from runtime event.
 *  Values of -1 mean "unknown" — keep the existing count. */
export const applySidebarCountsAtom = atom(
  null,
  (get, set, counts: {
    active: number; inbox: number; trash: number;
    uncategorized: number; untagged: number; duplicates: number;
  }) => {
    const patches: Record<string, number> = {};
    if (counts.active >= 0) patches['system:active'] = counts.active;
    if (counts.inbox >= 0) patches['system:inbox'] = counts.inbox;
    if (counts.trash >= 0) patches['system:trash'] = counts.trash;
    if (counts.uncategorized >= 0) patches['system:uncategorized'] = counts.uncategorized;
    if (counts.untagged >= 0) patches['system:untagged'] = counts.untagged;
    if (counts.duplicates >= 0) patches['system:duplicates'] = counts.duplicates;

    if (Object.keys(patches).length === 0) return;

    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => {
        const newCount = patches[n.id];
        return newCount !== undefined ? { ...n, count: newCount } : n;
      }),
    );
  },
);

/** Remove a confirmed folder deletion, including every descendant in the tree snapshot. */
export const removeFolderNodesAtom = atom(
  null,
  (get, set, folderNodeIds: Iterable<string>) => {
    const removed = new Set(folderNodeIds);
    set(sidebarNodesAtom, get(sidebarNodesAtom).filter((n) => !removed.has(n.id)));
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

// ── Batch delta atoms (used by runtime settle) ───────────────────

/** Apply folder parent changes from state_changed event. */
export const applyFolderParentChangesAtom = atom(
  null,
  (get, set, changes: Array<[number, number | null]>) => {
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => {
        if (n.kind !== 'folder') return n;
        for (const [fid, newParentId] of changes) {
          if (n.id === `folder:${fid}`) {
            return { ...n, parent_id: newParentId != null ? `folder:${newParentId}` : 'section:folders' };
          }
        }
        return n;
      }),
    );
  },
);

/** Apply folder sort order changes from state_changed event. */
export const applyFolderOrderChangesAtom = atom(
  null,
  (get, set, changes: Array<[number, number]>) => {
    const orderMap = new Map(changes.map(([id, order]) => [`folder:${id}`, order]));
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => {
        const newOrder = orderMap.get(n.id);
        return newOrder != null ? { ...n, sort_order: newOrder } : n;
      }),
    );
  },
);

/** Apply smart folder parent changes from state_changed event. */
export const applySmartFolderParentChangesAtom = atom(
  null,
  (get, set, changes: Array<[number, number | null]>) => {
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => {
        if (n.kind !== 'smart_folder') return n;
        for (const [sfId, newParentId] of changes) {
          if (n.id === `smart:${sfId}`) {
            return { ...n, parent_id: newParentId != null ? `smart:${newParentId}` : 'section:smart_folders' };
          }
        }
        return n;
      }),
    );
  },
);

/** A sidebar node patch from the backend runtime event. */
interface SidebarNodePatchPayload {
  node_id: string;
  removed?: boolean;
  upsert?: boolean;
  kind?: string;
  parent_id?: string | null;
  name?: string;
  icon?: string | null;
  color?: string | null;
  sort_order?: number | null;
  count?: number | null;
  selectable?: boolean;
  freshness?: string;
  meta_json?: string | null;
}

function parseMetaJson(metaJson: string | null | undefined): Record<string, unknown> | null {
  if (metaJson == null) return null;
  try {
    const parsed = JSON.parse(metaJson);
    return parsed && typeof parsed === 'object' ? parsed as Record<string, unknown> : null;
  } catch {
    return null;
  }
}

/** Apply sidebar node patches from state_changed event.
 *  Handles: partial updates, removals, and full upserts (insert-or-replace). */
export const applySidebarNodePatchesAtom = atom(
  null,
  (get, set, patches: SidebarNodePatchPayload[]) => {
    const removeIds = new Set(patches.filter((p) => p.removed).map((p) => p.node_id));
    const upserts = patches.filter((p) => p.upsert && !p.removed);
    const updates = patches.filter((p) => !p.upsert && !p.removed);
    const patchMap = new Map(updates.map((p) => [p.node_id, p]));

    let nodes = get(sidebarNodesAtom);

    // 1. Remove
    if (removeIds.size > 0) {
      nodes = nodes.filter((n) => !removeIds.has(n.id));
    }

    // 2. Patch existing nodes
    if (patchMap.size > 0) {
      nodes = nodes.map((n) => {
        const patch = patchMap.get(n.id);
        if (!patch) return n;
        const updated = { ...n };
        if (patch.name !== undefined) updated.name = patch.name;
        if ('icon' in patch) updated.icon = patch.icon;
        if ('color' in patch) updated.color = patch.color;
        if (patch.count !== undefined) updated.count = patch.count;
        if (patch.sort_order !== undefined) updated.sort_order = patch.sort_order;
        if (patch.freshness !== undefined) updated.freshness = patch.freshness;
        if ('meta_json' in patch) updated.meta = parseMetaJson(patch.meta_json);
        return updated;
      });
    }

    // 3. Upsert new nodes (insert-or-replace)
    if (upserts.length > 0) {
      const existingIds = new Set(nodes.map((n) => n.id));
      for (const u of upserts) {
        const node: SidebarNodeDto = {
          id: u.node_id,
          kind: u.kind ?? 'folder',
          parent_id: u.parent_id ?? null,
          name: u.name ?? '',
          icon: u.icon,
          color: u.color,
          sort_order: u.sort_order,
          count: u.count ?? 0,
          freshness: u.freshness ?? 'exact',
          selectable: u.selectable ?? true,
          meta: parseMetaJson(u.meta_json),
        };
        if (existingIds.has(u.node_id)) {
          // Replace existing
          nodes = nodes.map((n) => (n.id === u.node_id ? node : n));
        } else {
          // Insert new
          nodes = [...nodes, node];
        }
      }
    }

    set(sidebarNodesAtom, nodes);
  },
);

/** Apply smart folder count deltas from compiler publish. */
export const applySmartFolderCountsAtom = atom(
  null,
  (get, set, counts: Array<[number, number]>) => {
    if (counts.length === 0) return;
    const countMap = new Map(counts.map(([id, count]) => [`smart:${id}`, count]));
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => {
        const newCount = countMap.get(n.id);
        return newCount !== undefined ? { ...n, count: newCount, freshness: 'exact' } : n;
      }),
    );
  },
);

/** Apply smart folder sort order changes from state_changed event. */
export const applySmartFolderOrderChangesAtom = atom(
  null,
  (get, set, changes: Array<[number, number]>) => {
    const orderMap = new Map(changes.map(([id, order]) => [`smart:${id}`, order]));
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => {
        const newOrder = orderMap.get(n.id);
        return newOrder != null ? { ...n, sort_order: newOrder } : n;
      }),
    );
  },
);
