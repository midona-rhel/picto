/**
 * Grid drag state — module-level singleton for cross-component drag coordination.
 *
 * Not React state: used by CanvasGrid (pointer events), Sidebar (drop targets),
 * SubfolderGrid (drop targets), and AppShell (re-import guard).
 */

import * as api from '../../platform/api';
import type { EntityTarget } from '../../shared/types/canonical';

// ── Types ──

export type DropTarget =
  | { kind: 'folder'; folderId: number; nodeId: string }
  | { kind: 'status'; status: number }
  | { kind: 'reorder'; orderedEntityIds: [number, number][] };

export interface GridDragState {
  active: boolean;
  hashes: string[];
  sourceScope: { kind: string; id?: number | null; key?: string | null } | null;
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
  dropTarget: DropTarget | null;
}

// ── State ──

let state: GridDragState = {
  active: false,
  hashes: [],
  sourceScope: null,
  startX: 0,
  startY: 0,
  currentX: 0,
  currentY: 0,
  dropTarget: null,
};

let nativeDragPending = false;
let nativeDragTimer: ReturnType<typeof setTimeout> | null = null;

type DragEndHandler = (hashes: string[], target: DropTarget | null) => void;
const dragEndHandlers: DragEndHandler[] = [];

type DragChangeHandler = () => void;
const dragChangeHandlers: DragChangeHandler[] = [];

function notifyChange() {
  for (const h of dragChangeHandlers) h();
}

// ── Public API ──

export function getDragState(): Readonly<GridDragState> {
  return state;
}

export function isDragActive(): boolean {
  return state.active;
}

export function isNativeDragPending(): boolean {
  return nativeDragPending;
}

export function startDrag(
  hashes: string[],
  x: number,
  y: number,
  sourceScope: GridDragState['sourceScope'],
) {
  state = {
    active: true,
    hashes,
    sourceScope,
    startX: x,
    startY: y,
    currentX: x,
    currentY: y,
    dropTarget: null,
  };
  notifyChange();
}

export function moveDrag(x: number, y: number) {
  if (!state.active) return;
  state = { ...state, currentX: x, currentY: y };

  // Detect drop target via elementFromPoint
  const el = document.elementFromPoint(x, y) as HTMLElement | null;
  let newTarget: DropTarget | null = null;
  if (el) {
    // Check for folder drop target (sidebar row or subfolder tile)
    const folderDropEl = el.closest('[data-folder-drop-id]') as HTMLElement | null;
    const statusDropEl = el.closest('[data-status-drop]') as HTMLElement | null;
    const folderHashEl = el.closest('[data-folder-hash]') as HTMLElement | null;

    if (folderDropEl) {
      const fid = parseInt(folderDropEl.dataset.folderDropId ?? '', 10);
      if (!isNaN(fid)) newTarget = { kind: 'folder', folderId: fid, nodeId: `folder:${fid}` };
    } else if (statusDropEl) {
      const status = parseInt(statusDropEl.dataset.statusDrop ?? '', 10);
      if (!isNaN(status)) newTarget = { kind: 'status', status };
    } else if (folderHashEl) {
      const nodeId = folderHashEl.dataset.folderHash ?? '';
      const fid = parseInt(nodeId.replace('folder:', ''), 10);
      if (!isNaN(fid)) newTarget = { kind: 'folder', folderId: fid, nodeId };
    }
  }

  const prev = state.dropTarget;
  const changed = JSON.stringify(prev) !== JSON.stringify(newTarget);
  if (changed) {
    state = { ...state, dropTarget: newTarget };
    // Update visual highlight on drop targets
    document.querySelectorAll('[data-drop-highlighted]').forEach((el) => el.removeAttribute('data-drop-highlighted'));
    if (newTarget) {
      let targetEl: Element | null = null;
      if (newTarget.kind === 'folder') {
        targetEl = document.querySelector(`[data-folder-drop-id="${newTarget.folderId}"]`)
          ?? document.querySelector(`[data-folder-hash="${newTarget.nodeId}"]`);
      } else if (newTarget.kind === 'status') {
        targetEl = document.querySelector(`[data-status-drop="${newTarget.status}"]`);
      }
      targetEl?.setAttribute('data-drop-highlighted', 'true');
    }
    notifyChange();
  }
}

export function setDropTarget(target: DropTarget | null) {
  if (!state.active) return;
  state = { ...state, dropTarget: target };
  notifyChange();
}

function clearDropHighlights() {
  document.querySelectorAll('[data-drop-highlighted]').forEach((el) => el.removeAttribute('data-drop-highlighted'));
}

export function endDrag() {
  if (!state.active) return;
  const { hashes, dropTarget, sourceScope } = state;
  state = { ...state, active: false, dropTarget: null };
  clearDropHighlights();
  notifyChange();
  for (const h of dragEndHandlers) h(hashes, dropTarget);
  if (dropTarget) {
    void executeDrop(hashes, dropTarget, sourceScope).catch((err) => console.error('[drag] drop failed:', err));
  }
}

export function cancelDrag() {
  state = { ...state, active: false, dropTarget: null };
  clearDropHighlights();
  notifyChange();
}

/** Register a handler called when drag ends (with hashes + drop target). Returns unsubscribe. */
export function onDragEnd(handler: DragEndHandler): () => void {
  dragEndHandlers.push(handler);
  return () => {
    const idx = dragEndHandlers.indexOf(handler);
    if (idx >= 0) dragEndHandlers.splice(idx, 1);
  };
}

/** Subscribe to drag state changes (active/target changed). Returns unsubscribe. */
export function onDragChange(handler: DragChangeHandler): () => void {
  dragChangeHandlers.push(handler);
  return () => {
    const idx = dragChangeHandlers.indexOf(handler);
    if (idx >= 0) dragChangeHandlers.splice(idx, 1);
  };
}

// ── Native drag-out ──

export function startNativeDrag(hashes: string[], iconDataUrl: string) {
  nativeDragPending = true;
  if (nativeDragTimer) clearTimeout(nativeDragTimer);
  nativeDragTimer = setTimeout(() => {
    nativeDragPending = false;
    nativeDragTimer = null;
  }, 30_000);
  void (window as any).picto?.webview?.startNativeDrag?.(hashes, iconDataUrl)?.catch?.(() => {});
  cancelDrag();
}

export function clearNativeDragPending() {
  nativeDragPending = false;
  if (nativeDragTimer) { clearTimeout(nativeDragTimer); nativeDragTimer = null; }
}

// ── Drop execution ──

export async function executeDrop(
  hashes: string[],
  target: DropTarget,
  sourceScope: GridDragState['sourceScope'],
): Promise<void> {
  const entityTarget: EntityTarget = {
    kind: 'entity_hashes',
    entity_hashes: hashes.filter((h) => !h.startsWith('folder:')),
  };
  const entityHashes = entityTarget.entity_hashes ?? [];
  if (entityHashes.length === 0) return;

  if (target.kind === 'folder') {
    await api.updateFolderMembership(entityTarget, target.folderId, 'add');
    // If source is inbox or trash, also set to active
    if (sourceScope?.key === 'inbox' || sourceScope?.key === 'trash') {
      await api.setEntityStatus(entityTarget, 1);
    }
  } else if (target.kind === 'status') {
    await api.setEntityStatus(entityTarget, target.status);
  } else if (target.kind === 'reorder') {
    if (sourceScope?.kind === 'folder' && sourceScope.id != null) {
      await api.reorderFolderMembers(sourceScope.id, target.orderedEntityIds);
    } else if (sourceScope?.kind === 'collection' && sourceScope.id != null) {
      // Collections use full ordered hash list
      // TODO: wire when collection reorder is needed
    }
  }
}
