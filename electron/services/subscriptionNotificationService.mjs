const SETTLE_DEBOUNCE_MS = 250;

function parseCoreJson(serialized) {
  return JSON.parse(serialized);
}

export function showSubscriptionSettlementNotification({ Notification, app, platform = process.platform, newItems }) {
  const count = Math.max(0, Math.trunc(Number(newItems) || 0));
  if (platform === 'darwin' && count > 0 && app.dock) {
    app.dock.setBadge(String(Math.min(9_999, count)));
  }
  if (!Notification.isSupported()) return;
  const body = count === 0
    ? 'All subscriptions finished. No new images were added.'
    : `${count.toLocaleString('en-US')} new image${count === 1 ? '' : 's'} ${count === 1 ? 'is' : 'are'} ready to review.`;
  new Notification({ title: 'Subscriptions finished', body }).show();
}

export function createSubscriptionNotificationService({
  Notification,
  app,
  invokeSerialized,
  getCurrentLibraryRoot,
  platform = process.platform,
}) {
  const batchRunIds = new Set();
  let batchActive = false;
  let activeLibraryRoot = null;
  let refreshPromise = null;
  let refreshQueued = false;
  let settleTimer = null;
  let stopped = false;

  async function read(command, args = {}) {
    return parseCoreJson(await invokeSerialized(command, args));
  }

  async function refreshOnce() {
    const libraryRoot = getCurrentLibraryRoot();
    if (!libraryRoot) return;
    if (libraryRoot !== activeLibraryRoot) {
      activeLibraryRoot = libraryRoot;
      batchRunIds.clear();
      batchActive = false;
    }

    const list = await read('subscriptions.list');
    const activeRuns = (list.subscriptions ?? []).flatMap((subscription) => (
      subscription.active_run_id != null && ['pending', 'running'].includes(subscription.status)
        ? [subscription.active_run_id]
        : []
    ));
    activeRuns.forEach((runId) => batchRunIds.add(runId));
    if (activeRuns.length > 0) {
      batchActive = true;
      return;
    }
    if (!batchActive || batchRunIds.size === 0) return;

    const completedRunIds = [...batchRunIds];
    batchRunIds.clear();
    batchActive = false;
    const activities = await Promise.allSettled(completedRunIds.map((runId) => (
      read('subscriptions.runs.get', { run_id: runId, source_item_limit: 1 })
    )));
    const newItems = activities.reduce((total, result) => (
      result.status === 'fulfilled'
        ? total + Math.max(0, Number(result.value?.summary?.counts?.ingested) || 0)
        : total
    ), 0);
    showSubscriptionSettlementNotification({ Notification, app, platform, newItems });
  }

  function refresh() {
    if (stopped) return Promise.resolve();
    if (refreshPromise) {
      refreshQueued = true;
      return refreshPromise;
    }
    refreshPromise = (async () => {
      do {
        refreshQueued = false;
        await refreshOnce();
      } while (refreshQueued && !stopped);
    })().catch((error) => {
      console.warn('[main] subscription notification refresh failed', error);
    }).finally(() => {
      refreshPromise = null;
    });
    return refreshPromise;
  }

  function handleNativeEvent(name, payload) {
    if (stopped || name !== 'library/changed') return;
    if (!payload?.resources?.some((resource) => resource === 'subscriptions' || resource === 'tasks')) return;
    if (settleTimer !== null) clearTimeout(settleTimer);
    settleTimer = setTimeout(() => {
      settleTimer = null;
      void refresh();
    }, SETTLE_DEBOUNCE_MS);
  }

  function stop() {
    stopped = true;
    if (settleTimer !== null) clearTimeout(settleTimer);
    settleTimer = null;
    batchRunIds.clear();
  }

  return { refresh, handleNativeEvent, stop };
}
