/**
 * Custom image drag system — avoids HTML5 drag API to prevent
 * Native drag interference. Uses mousedown/mousemove/mouseup.
 *
 * Folder drop targets register via `data-folder-drop-id` attribute.
 * Also tracks pending native drag hashes for Electron startDrag integration.
 */

import { useSyncExternalStore } from 'react';

export interface ImageDragState {
  hashes: string[];
  thumbnailUrls: string[];
  x: number;
  y: number;
  dropTargetFolderId: number | null;
}

let _state: ImageDragState | null = null;
const _listeners = new Set<() => void>();
function _notify() { _listeners.forEach(fn => fn()); }

/** Module-level selected hashes — avoids passing Set prop to every tile. */
let _selectedHashesRef: Set<string> = new Set();

type DropHandler = (result: { hashes: string[]; folderId: number }) => void;
let _onDropHandler: DropHandler | null = null;

// PBI-053: Session-tracked native drag state with timeout guard.
interface NativeDragSession {
  sessionId: number;
  hashes: string[];
  startedAt: number;
  timeoutId: ReturnType<typeof setTimeout> | null;
}
let _nativeDragSession: NativeDragSession | null = null;
const _nativeDragEndListeners = new Set<() => void>();
let _nextSessionId = 1;
const NATIVE_DRAG_TIMEOUT_MS = 30000; // 30s guard for abnormal termination

function _notifyNativeDragEnd() {
  _nativeDragEndListeners.forEach((fn) => fn());
}

export const imageDrag = {
  setSelectedHashes(hashes: Set<string>) { _selectedHashesRef = hashes; },
  getSelectedHashes() { return _selectedHashesRef; },

  start(hashes: string[], thumbnailUrls: string[], x: number, y: number) {
    _state = { hashes, thumbnailUrls, x, y, dropTargetFolderId: null };
    document.body.style.userSelect = 'none';
    document.body.style.cursor = 'grabbing';
    _notify();
  },

  move(x: number, y: number) {
    if (!_state) return;
    const el = document.elementFromPoint(x, y);
    const folderEl = el?.closest('[data-folder-drop-id]') as HTMLElement | null;
    const folderId = folderEl ? parseInt(folderEl.dataset.folderDropId!, 10) : null;
    _state = {
      ..._state,
      x,
      y,
      dropTargetFolderId: folderId != null && !isNaN(folderId) ? folderId : null,
    };
    _notify();
  },

  end() {
    if (!_state) return;
    const { hashes, dropTargetFolderId } = _state;
    _state = null;
    document.body.style.userSelect = '';
    document.body.style.cursor = '';
    _notify();
    if (dropTargetFolderId != null && _onDropHandler) {
      _onDropHandler({ hashes, folderId: dropTargetFolderId });
    }
  },

  /** Clean up drag state without triggering folder drop handler (used after native drag ends). */
  forceEnd() {
    if (!_state && !_nativeDragSession) return;
    _state = null;
    // PBI-053: Also clear native drag session.
    if (_nativeDragSession?.timeoutId) clearTimeout(_nativeDragSession.timeoutId);
    if (_nativeDragSession) {
      _nativeDragSession = null;
      _notifyNativeDragEnd();
    }
    document.body.style.userSelect = '';
    document.body.style.cursor = '';
    _notify();
  },

  /** Register a handler for drops onto folders. Returns cleanup function. */
  onDrop(handler: DropHandler) {
    _onDropHandler = handler;
    return () => {
      if (_onDropHandler === handler) _onDropHandler = null;
    };
  },

  // PBI-053: Session-based native drag tracking with timeout guard.
  startNativeDragSession(hashes: string[]): number {
    // Clear any stale session.
    if (_nativeDragSession?.timeoutId) clearTimeout(_nativeDragSession.timeoutId);
    if (_nativeDragSession) {
      _nativeDragSession = null;
      _notifyNativeDragEnd();
    }
    const sessionId = _nextSessionId++;
    const timeoutId = setTimeout(() => {
      if (_nativeDragSession?.sessionId === sessionId) {
        if (import.meta.env.DEV) console.log('[imageDrag] native_drag_cleared_timeout', sessionId);
        _nativeDragSession = null;
        _notifyNativeDragEnd();
      }
    }, NATIVE_DRAG_TIMEOUT_MS);
    _nativeDragSession = { sessionId, hashes, startedAt: Date.now(), timeoutId };
    if (import.meta.env.DEV) console.log('[imageDrag] native_drag_started', sessionId, hashes.length);
    return sessionId;
  },
  getPendingNativeDragHashes(): string[] | null { return _nativeDragSession?.hashes ?? null; },
  clearNativeDragSession(sessionId?: number) {
    if (sessionId != null && _nativeDragSession?.sessionId !== sessionId) return; // Idempotent
    if (_nativeDragSession?.timeoutId) clearTimeout(_nativeDragSession.timeoutId);
    if (import.meta.env.DEV && _nativeDragSession) console.log('[imageDrag] native_drag_cleared', _nativeDragSession.sessionId);
    const hadSession = _nativeDragSession != null;
    _nativeDragSession = null;
    if (hadSession) _notifyNativeDragEnd();
  },
  onNativeDragEnd(handler: () => void) {
    _nativeDragEndListeners.add(handler);
    return () => { _nativeDragEndListeners.delete(handler); };
  },
  /** @deprecated Use startNativeDragSession instead. */
  setPendingNativeDragHashes(hashes: string[]) { this.startNativeDragSession(hashes); },
  /** @deprecated Use clearNativeDragSession instead. */
  clearPendingNativeDragHashes() { this.clearNativeDragSession(); },

  getDropTargetFolderId(): number | null {
    return _state?.dropTargetFolderId ?? null;
  },

  get active() { return _state != null; },
};

/** Full drag state (re-renders on every move — use for DragGhost). */
export function useImageDrag(): ImageDragState | null {
  return useSyncExternalStore(
    (cb) => { _listeners.add(cb); return () => { _listeners.delete(cb); }; },
    () => _state,
  );
}

/** Just the drop target folder ID (re-renders only when target changes). */
export function useImageDragDropTarget(): number | null {
  return useSyncExternalStore(
    (cb) => { _listeners.add(cb); return () => { _listeners.delete(cb); }; },
    () => _state?.dropTargetFolderId ?? null,
  );
}
