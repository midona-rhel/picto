import { normalizeInvokeError } from '../../platform/ipc';

/**
 * Rejection message of a library open the host cancelled: an explicit close,
 * a newer open superseding it, or the app quitting. Mirrors
 * `electron/libraryOpenCancellation.mjs`. The gate's `library-open-failed`
 * event already drives the UI for these, so callers only have to stay quiet.
 */
export const LIBRARY_OPEN_CANCELLED = 'Library open was cancelled';

/** Recognizes the sentinel behind the `ipcRenderer.invoke` error prefix. */
export function isLibraryOpenCancelled(error: unknown): boolean {
  return normalizeInvokeError(error).message === LIBRARY_OPEN_CANCELLED;
}
