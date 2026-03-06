const BLURHASH_BACKFILL_BATCH = 96;
const BLURHASH_BACKFILL_ACTIVE_DELAY_MS = 750;
const BLURHASH_BACKFILL_IDLE_DELAY_MS = 5000;

export function createMediaMaintenanceService({ invoke, isDev, getCurrentLibraryRoot }) {
  let blurhashBackfillTimer = null;
  let blurhashBackfillInFlight = false;

  function scheduleBlurhashBackfill(nextDelayMs) {
    if (blurhashBackfillTimer != null) {
      clearTimeout(blurhashBackfillTimer);
      blurhashBackfillTimer = null;
    }
    blurhashBackfillTimer = setTimeout(runBlurhashBackfillTick, nextDelayMs);
  }

  function stopBlurhashBackfill() {
    if (blurhashBackfillTimer != null) {
      clearTimeout(blurhashBackfillTimer);
      blurhashBackfillTimer = null;
    }
    blurhashBackfillInFlight = false;
  }

  function startBlurhashBackfill() {
    stopBlurhashBackfill();
    scheduleBlurhashBackfill(1200);
  }

  async function runBlurhashBackfillTick() {
    if (blurhashBackfillInFlight || !getCurrentLibraryRoot()) return;
    blurhashBackfillInFlight = true;
    try {
      const result = await invoke('backfill_missing_blurhashes', { limit: BLURHASH_BACKFILL_BATCH });
      const remaining = Number(result?.remaining ?? 0);
      if (isDev && Number(result?.processed ?? 0) > 0) {
        console.info('[blurhash] backfill batch', result);
      }
      scheduleBlurhashBackfill(remaining > 0 ? BLURHASH_BACKFILL_ACTIVE_DELAY_MS : BLURHASH_BACKFILL_IDLE_DELAY_MS);
    } catch (error) {
      if (isDev) console.warn('[blurhash] backfill failed', error);
      scheduleBlurhashBackfill(BLURHASH_BACKFILL_IDLE_DELAY_MS);
    } finally {
      blurhashBackfillInFlight = false;
    }
  }

  return {
    startBlurhashBackfill,
    stopBlurhashBackfill,
  };
}
