import { dragController } from '../../controllers/dragController';
import type { FolderReorderMove } from '../../platform/folderApi';

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

let state: GridDragState = {
  active: false, hashes: [], sourceScope: null,
  startX: 0, startY: 0, currentX: 0, currentY: 0, dropTarget: null,
};
let highlightedElement: HTMLElement | null = null;
let nativeDragPending = false;
let nativeDragTimer: ReturnType<typeof setTimeout> | null = null;
let internalDragOrigin = false;

export const getDragState = (): Readonly<GridDragState> => state;
export const isDragActive = () => state.active;
export const isNativeDragPending = () => nativeDragPending;
export const isInternalDragOrigin = () => internalDragOrigin;
export const setInternalDragOrigin = (value: boolean) => { internalDragOrigin = value; };

export function startDrag(hashes: string[], x: number, y: number, sourceScope: GridDragState['sourceScope']) {
  state = { active: true, hashes, sourceScope, startX: x, startY: y, currentX: x, currentY: y, dropTarget: null };
  document.documentElement.dataset.gridDragActive = 'true';
}

interface ResolvedDropTarget {
  target: Extract<DropTarget, { kind: 'folder' | 'status' }>;
  element: HTMLElement;
}

export function resolveDropTarget(element: HTMLElement | null): ResolvedDropTarget | null {
  if (!element) return null;
  const folderDrop = element.closest<HTMLElement>('[data-folder-drop-id]');
  const folderId = Number.parseInt(folderDrop?.dataset.folderDropId ?? '', 10);
  if (folderDrop && !Number.isNaN(folderId)) {
    return { target: { kind: 'folder', folderId, nodeId: `folder:${folderId}` }, element: folderDrop };
  }
  const statusDrop = element.closest<HTMLElement>('[data-status-drop]');
  const status = Number.parseInt(statusDrop?.dataset.statusDrop ?? '', 10);
  if (statusDrop && !Number.isNaN(status)) return { target: { kind: 'status', status }, element: statusDrop };
  const folderTile = element.closest<HTMLElement>('[data-folder-hash]');
  const nodeId = folderTile?.dataset.folderHash ?? '';
  const tileFolderId = Number.parseInt(nodeId.replace('folder:', ''), 10);
  return folderTile && !Number.isNaN(tileFolderId)
    ? { target: { kind: 'folder', folderId: tileFolderId, nodeId }, element: folderTile }
    : null;
}

function sameTarget(left: DropTarget | null, right: DropTarget | null) {
  if (left?.kind !== right?.kind) return false;
  if (!left || !right) return true;
  if (left.kind === 'folder' && right.kind === 'folder') return left.folderId === right.folderId;
  if (left.kind === 'status' && right.kind === 'status') return left.status === right.status;
  return left === right;
}

function highlight(element: HTMLElement | null) {
  if (highlightedElement === element) return;
  highlightedElement?.removeAttribute('data-drop-highlighted');
  highlightedElement = element;
  highlightedElement?.setAttribute('data-drop-highlighted', 'true');
}

export function moveDrag(x: number, y: number) {
  if (!state.active) return;
  state.currentX = x;
  state.currentY = y;
  const resolved = resolveDropTarget(document.elementFromPoint(x, y) as HTMLElement | null);
  highlight(resolved?.element ?? null);
  if (!sameTarget(state.dropTarget, resolved?.target ?? null)) state.dropTarget = resolved?.target ?? null;
}

export function setDropTarget(target: DropTarget | null) {
  if (state.active) state.dropTarget = target;
}

export function cancelDrag() {
  state.active = false;
  state.dropTarget = null;
  highlight(null);
  delete document.documentElement.dataset.gridDragActive;
}

export function endDrag() {
  if (!state.active) return;
  const { hashes, dropTarget, sourceScope } = state;
  cancelDrag();
  if (dropTarget) void dragController.executeDrop(hashes, dropTarget, sourceScope)
    .catch((error) => console.error('[drag] drop failed:', error));
}

export function startNativeDrag(hashes: string[], iconDataUrl: string) {
  nativeDragPending = internalDragOrigin = true;
  if (nativeDragTimer) clearTimeout(nativeDragTimer);
  nativeDragTimer = setTimeout(() => {
    nativeDragPending = internalDragOrigin = false;
    nativeDragTimer = null;
  }, 3_000);
  (window as any).picto?.webview?.startNativeDrag?.(hashes, iconDataUrl);
  cancelDrag();
}
