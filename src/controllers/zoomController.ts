/**
 * Zoom controller — controls app-wide zoom via CSS transform.
 * Works without preload/webFrame access.
 */

const ZOOM_STEP = 0.05;
const ZOOM_MIN = 0.5;
const ZOOM_MAX = 2.0;
const STORAGE_KEY = 'picto-zoom-factor';

type ZoomListener = (factor: number) => void;
const listeners = new Set<ZoomListener>();

let currentZoom = parseFloat(localStorage.getItem(STORAGE_KEY) ?? '1');
applyZoom(currentZoom);

function applyZoom(factor: number) {
  document.documentElement.style.setProperty('zoom', String(factor));
}

export const zoomController = {
  getZoom(): number {
    return currentZoom;
  },

  setZoom(factor: number) {
    currentZoom = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(factor * 100) / 100));
    applyZoom(currentZoom);
    localStorage.setItem(STORAGE_KEY, String(currentZoom));
    for (const listener of listeners) listener(currentZoom);
    return currentZoom;
  },

  /** Notify on zoom changes. Returns an unsubscribe function. */
  subscribe(listener: ZoomListener): () => void {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },

  zoomIn() {
    return this.setZoom(currentZoom + ZOOM_STEP);
  },

  zoomOut() {
    return this.setZoom(currentZoom - ZOOM_STEP);
  },

  resetZoom() {
    return this.setZoom(1.0);
  },
};
