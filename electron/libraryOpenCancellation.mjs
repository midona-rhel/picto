// The cancellation contract of the native library open, kept free of the
// native binding so that host code and its tests can share it.

/** Rejection message of an `openLibrary()` cancelled by `closeLibrary()`. */
export const LIBRARY_OPEN_CANCELLED = 'Library open was cancelled';

export function isLibraryOpenCancelled(error) {
  return error?.message === LIBRARY_OPEN_CANCELLED;
}
