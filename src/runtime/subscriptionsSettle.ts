import { listen } from '../platform/ipc';

const workspaceRefreshCallbacks = new Set<() => void>();
const authRefreshCallbacks = new Set<() => void>();

function trigger(callbacks: Set<() => void>) {
  for (const callback of callbacks) {
    try {
      callback();
    } catch (error) {
      console.error('subscription settle callback failed', error);
    }
  }
}

export function registerSubscriptionsWorkspaceRefresh(callback: () => void): () => void {
  workspaceRefreshCallbacks.add(callback);
  return () => {
    workspaceRefreshCallbacks.delete(callback);
  };
}

export function registerAuthWorkspaceRefresh(callback: () => void): () => void {
  authRefreshCallbacks.add(callback);
  return () => {
    authRefreshCallbacks.delete(callback);
  };
}

export function startSubscriptionsSettle(): () => void {
  let cancelled = false;
  const unlistenPromise = listen<{ changes?: { domains?: string[] } }>(
    'runtime/state_changed',
    ({ payload }) => {
      if (cancelled) return;
      const domains = payload.changes?.domains ?? [];
      if (!domains.includes('subscriptions')) return;
      trigger(workspaceRefreshCallbacks);
      trigger(authRefreshCallbacks);
    },
  );

  return () => {
    cancelled = true;
    unlistenPromise.then((fn) => fn()).catch(() => {});
  };
}
