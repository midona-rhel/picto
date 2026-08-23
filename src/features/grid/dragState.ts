/**
 * Grid drag state — module-level singleton for cross-component drag coordination.
 *
 * Not React state: used by CanvasGrid (pointer events), Sidebar (drop targets),
 * SubfolderGrid (drop targets), and AppShell (re-import guard).
 */

import { dragController } from '../../controllers/dragController';
import type { ItemScope } from '../../shared/types/generated/application/ItemScope';

// ── Types ──

export type DropTarget =
  | { kind: 'folder'; folderId: number; nodeId: string }
  | { kind: 'status'; status: number }
  | { kind: 'reorder'; orderedItemIds: number[] };

export interface GridDragState {
  active: boolean;
  ownerId: number | null;
  itemIds: number[];
  sourceScope: ItemScope | null;
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
  dropTarget: DropTarget | null;
}

// ── State ──

let state: GridDragState = {
  active: false,
  ownerId: null,
  itemIds: [],
  sourceScope: null,
  startX: 0,
  startY: 0,
  currentX: 0,
  currentY: 0,
  dropTarget: null,
};

let nativeDragPending = false;
let nativeDragTimer: ReturnType<typeof setTimeout> | null = null;

let highlightedDropElement: HTMLElement | null = null;
let nextDragOwnerId = 1;

// ── Public API ──

export function getDragState(): Readonly<GridDragState> {
  return state;
}

export function isDragActive(): boolean {
  return state.active;
}

export function createDragOwnerId(): number {
  return nextDragOwnerId++;
}

export function isDragOwnedBy(ownerId: number): boolean {
  return state.active && state.ownerId === ownerId;
}

export function isNativeDragPending(): boolean {
  return nativeDragPending;
}

export function startDrag(
  itemIds: number[],
  x: number,
  y: number,
  sourceScope: GridDragState['sourceScope'],
  ownerId: number | null = null,
) {
  state = {
    active: true,
    ownerId,
    itemIds,
    sourceScope,
    startX: x,
    startY: y,
    currentX: x,
    currentY: y,
    dropTarget: null,
  };
}

export function moveDrag(x: number, y: number) {
  if (!state.active) return;
  state = { ...state, currentX: x, currentY: y };

  // Detect drop target via elementFromPoint
  const el = document.elementFromPoint(x, y) as HTMLElement | null;
  const resolved = resolveDropTarget(el);
  const newTarget = resolved?.target ?? null;

  const prev = state.dropTarget;
  const changed = !sameDropTarget(prev, newTarget);
  setHighlightedDropElement(resolved?.element ?? null);
  if (changed) {
    state = { ...state, dropTarget: newTarget };
  }
}

interface ResolvedDropTarget {
  target: Extract<DropTarget, { kind: 'folder' | 'status' }>;
  element: HTMLElement;
}

/** Resolve the semantic target and the exact hovered drop surface together. */
export function resolveDropTarget(element: HTMLElement | null): ResolvedDropTarget | null {
  if (!element) return null;

  const folderDropElement = element.closest<HTMLElement>('[data-folder-drop-id]');
  if (folderDropElement) {
    const folderId = Number.parseInt(folderDropElement.dataset.folderDropId ?? '', 10);
    if (!Number.isNaN(folderId)) {
      return {
        target: { kind: 'folder', folderId, nodeId: `folder:${folderId}` },
        element: folderDropElement,
      };
    }
  }

  const statusDropElement = element.closest<HTMLElement>('[data-status-drop]');
  if (statusDropElement) {
    const status = Number.parseInt(statusDropElement.dataset.statusDrop ?? '', 10);
    if (!Number.isNaN(status)) {
      return { target: { kind: 'status', status }, element: statusDropElement };
    }
  }

  const folderTileElement = element.closest<HTMLElement>('[data-folder-hash]');
  const nodeId = folderTileElement?.dataset.folderHash ?? '';
  const folderId = Number.parseInt(nodeId.replace('folder:', ''), 10);
  if (folderTileElement && !Number.isNaN(folderId)) {
    return {
      target: { kind: 'folder', folderId, nodeId },
      element: folderTileElement,
    };
  }

  return null;
}

function sameDropTarget(left: DropTarget | null, right: DropTarget | null): boolean {
  if (left?.kind !== right?.kind) return false;
  if (!left || !right) return true;
  if (left.kind === 'folder' && right.kind === 'folder') return left.folderId === right.folderId;
  if (left.kind === 'status' && right.kind === 'status') return left.status === right.status;
  return left === right;
}

function setHighlightedDropElement(next: HTMLElement | null) {
  if (highlightedDropElement === next) return;
  highlightedDropElement?.removeAttribute('data-drop-highlighted');
  highlightedDropElement = next;
  highlightedDropElement?.setAttribute('data-drop-highlighted', 'true');
}

export function setDropTarget(target: DropTarget | null) {
  if (!state.active) return;
  state = { ...state, dropTarget: target };
}

function clearDropHighlights() {
  setHighlightedDropElement(null);
}

export function endDrag() {
  if (!state.active) return;
  const { itemIds, dropTarget, sourceScope } = state;
  state = { ...state, active: false, ownerId: null, dropTarget: null };
  clearDropHighlights();
  if (dropTarget) {
    void dragController.executeDrop(itemIds, dropTarget, sourceScope).catch((err) => console.error('[drag] drop failed:', err));
  }
}

export function cancelDrag() {
  state = { ...state, active: false, ownerId: null, dropTarget: null };
  clearDropHighlights();
}

// ── Internal drag origin tracking (prevents import overlay during app-originated drags) ──

let internalDragOrigin = false;
export function setInternalDragOrigin(v: boolean) { internalDragOrigin = v; }
export function isInternalDragOrigin() { return internalDragOrigin; }

// Saved drag data for restoring internal drag when cursor re-enters
let savedDragItemIds: number[] = [];
let savedDragScope: ItemScope | null = null;
let savedDragOwnerId: number | null = null;

// ── Native drag-out ──

export function startNativeDrag(fileHashes: string[], iconDataUrl: string) {
  // Save drag data before cancelling so we can restore on re-entry
  savedDragItemIds = [...state.itemIds];
  savedDragScope = state.sourceScope;
  savedDragOwnerId = state.ownerId;

  nativeDragPending = true;
  internalDragOrigin = true;
  if (nativeDragTimer) clearTimeout(nativeDragTimer);
  nativeDragTimer = setTimeout(() => {
    nativeDragPending = false;
    internalDragOrigin = false;
    savedDragItemIds = [];
    savedDragScope = null;
    savedDragOwnerId = null;
    nativeDragTimer = null;
  }, 3_000);
  (window as any).picto?.webview?.startNativeDrag?.(fileHashes, iconDataUrl);
  cancelDrag();
}

/** Restore internal drag from a native drag that re-entered the app window. */
export function restoreInternalDrag(x: number, y: number) {
  if (savedDragItemIds.length === 0) return false;
  nativeDragPending = false;
  internalDragOrigin = false;
  if (nativeDragTimer) { clearTimeout(nativeDragTimer); nativeDragTimer = null; }
  startDrag(savedDragItemIds, x, y, savedDragScope, savedDragOwnerId);
  savedDragItemIds = [];
  savedDragScope = null;
  savedDragOwnerId = null;
  return true;
}
