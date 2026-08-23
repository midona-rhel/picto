/**
 * Grid drag state — module-level singleton for cross-component drag coordination.
 *
 * Not React state: used by CanvasGrid (pointer events), Sidebar (drop targets),
 * SubfolderGrid (drop targets), and AppShell (re-import guard).
 */

import { dragController } from '../../controllers/dragController';
import type { FolderReorderMove } from '../../platform/folderApi';

// ── Types ──

export type DropTarget =
  | { kind: 'folder'; folderId: number; nodeId: string }
  | { kind: 'status'; status: number }
  | { kind: 'reorder'; moves: FolderReorderMove[] };

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
let highlightedDropElement: HTMLElement | null = null;

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
  const resolved = resolveDropTarget(el);
  const newTarget = resolved?.target ?? null;

  const prev = state.dropTarget;
  const changed = !sameDropTarget(prev, newTarget);
  setHighlightedDropElement(resolved?.element ?? null);
  if (changed) {
    state = { ...state, dropTarget: newTarget };
    notifyChange();
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
  notifyChange();
}

function clearDropHighlights() {
  setHighlightedDropElement(null);
}

export function endDrag() {
  if (!state.active) return;
  const { hashes, dropTarget, sourceScope } = state;
  state = { ...state, active: false, dropTarget: null };
  clearDropHighlights();
  notifyChange();
  for (const h of dragEndHandlers) h(hashes, dropTarget);
  if (dropTarget) {
    void dragController.executeDrop(hashes, dropTarget, sourceScope).catch((err) => console.error('[drag] drop failed:', err));
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

// ── Internal drag origin tracking (prevents import overlay during app-originated drags) ──

let internalDragOrigin = false;
export function setInternalDragOrigin(v: boolean) { internalDragOrigin = v; }
export function isInternalDragOrigin() { return internalDragOrigin; }

// Saved drag data for restoring internal drag when cursor re-enters
let savedDragHashes: string[] = [];
let savedDragScope: { kind: string; id?: number | null; key?: string | null } | null = null;

export function getSavedDragHashes() { return savedDragHashes; }
export function getSavedDragScope() { return savedDragScope; }

// ── Native drag-out ──

export function startNativeDrag(hashes: string[], iconDataUrl: string) {
  // Save drag data before cancelling so we can restore on re-entry
  savedDragHashes = state.hashes.length > 0 ? [...state.hashes] : [...hashes];
  savedDragScope = state.sourceScope;

  nativeDragPending = true;
  internalDragOrigin = true;
  if (nativeDragTimer) clearTimeout(nativeDragTimer);
  nativeDragTimer = setTimeout(() => {
    nativeDragPending = false;
    internalDragOrigin = false;
    savedDragHashes = [];
    savedDragScope = null;
    nativeDragTimer = null;
  }, 3_000);
  (window as any).picto?.webview?.startNativeDrag?.(hashes, iconDataUrl);
  cancelDrag();
}

/** Restore internal drag from a native drag that re-entered the app window. */
export function restoreInternalDrag(x: number, y: number) {
  if (savedDragHashes.length === 0) return false;
  nativeDragPending = false;
  internalDragOrigin = false;
  if (nativeDragTimer) { clearTimeout(nativeDragTimer); nativeDragTimer = null; }
  startDrag(savedDragHashes, x, y, savedDragScope);
  savedDragHashes = [];
  savedDragScope = null;
  return true;
}

export function clearNativeDragPending() {
  nativeDragPending = false;
  internalDragOrigin = false;
  if (nativeDragTimer) { clearTimeout(nativeDragTimer); nativeDragTimer = null; }
}
