/**
 * The app badge is the number of items in the Inbox, like an unread count.
 * It follows every library change (imports, subscription runs, accept,
 * reject, undo) and the library being switched or closed; it is not cleared
 * by looking at the app.
 *
 * Reading the count is a bitmap length in the resident projection, so a
 * refresh per settled change is cheap. Changes are coalesced so a burst of
 * ingest batches sets the badge once.
 */
const CHANGE_DEBOUNCE_MS = 200;
const MAX_BADGE = 9_999;

export function createInboxBadgeService({
  app,
  invokeSerialized,
  getCurrentLibraryRoot,
}) {
  let shown = null;
  let timer = null;
  let running = null;
  let queued = false;
  let stopped = false;

  function show(count) {
    const next = Math.max(0, Math.min(MAX_BADGE, Math.trunc(Number(count) || 0)));
    if (next === shown) return;
    shown = next;
    // macOS dock and Linux launchers; a no-op where the platform has no badge.
    app.setBadgeCount(next);
  }

  async function refreshOnce() {
    const libraryRoot = getCurrentLibraryRoot();
    if (!libraryRoot) {
      show(0);
      return;
    }
    const count = JSON.parse(await invokeSerialized('library.inbox_count', {}));
    // A library switched meanwhile gets its own refresh.
    if (!stopped && getCurrentLibraryRoot() === libraryRoot) show(count);
  }

  function refresh() {
    if (stopped) return Promise.resolve();
    if (running) {
      queued = true;
      return running;
    }
    running = (async () => {
      do {
        queued = false;
        await refreshOnce();
      } while (queued && !stopped);
    })().catch((error) => {
      console.warn('[main] inbox badge refresh failed', error);
    }).finally(() => {
      running = null;
    });
    return running;
  }

  function handleNativeEvent(name) {
    if (stopped || name !== 'library/changed') return;
    if (timer !== null) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      void refresh();
    }, CHANGE_DEBOUNCE_MS);
  }

  function stop() {
    stopped = true;
    if (timer !== null) clearTimeout(timer);
    timer = null;
    show(0);
  }

  return { refresh, handleNativeEvent, stop };
}
