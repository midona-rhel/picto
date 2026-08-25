import { useCallback, useEffect, useMemo, useState } from 'react';
import { authController } from '../../controllers/authController';
import { registerAuthWorkspaceRefresh } from '../../runtime/subscriptionsSettle';
import type { AuthSessionState, OnlyFansManualAuthInput } from '../../shared/types/subscriptions';
import type { AuthSiteSnapshot, AuthWorkspaceSnapshot } from '../../shared/types/subscriptionsWorkspace';
import { AuthSiteDetail } from './components/AuthSiteDetail';
import { AuthSitesSidebar } from './components/AuthSitesSidebar';
import styles from './AuthWorkspace.module.css';

const IDLE_SESSION: AuthSessionState = {
  site_category: null,
  status: 'idle',
  title: null,
  current_url: null,
  message: null,
};

export function AuthWorkspace({
  hideSidebar = false,
  onSitesLoaded,
  externalSelectedSiteId,
  onSelectSite,
}: {
  hideSidebar?: boolean;
  onSitesLoaded?: (sites: AuthSiteSnapshot[]) => void;
  externalSelectedSiteId?: string | null;
  onSelectSite?: (siteId: string) => void;
} = {}) {
  const [snapshot, setSnapshot] = useState<AuthWorkspaceSnapshot | null>(null);
  const [selectedSiteId, setSelectedSiteId] = useState<string | null>(null);
  const [session, setSession] = useState<AuthSessionState>(IDLE_SESSION);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  // Sync external selection when provided
  useEffect(() => {
    if (externalSelectedSiteId != null) setSelectedSiteId(externalSelectedSiteId);
  }, [externalSelectedSiteId]);

  const refresh = useCallback(async (preserveSelection = true) => {
    const next = await authController.loadWorkspaceSnapshot();
    setSnapshot(next);
    onSitesLoaded?.(next.sites);
    setSelectedSiteId((current) => {
      if (preserveSelection && current && next.sites.some((site) => site.site.id === current)) return current;
      return next.sites[0]?.site.id ?? null;
    });
  }, [onSitesLoaded]);

  useEffect(() => {
    void refresh();
  }, []);

  const selectedEntry = useMemo(
    () => snapshot?.sites.find((entry) => entry.site.id === selectedSiteId) ?? null,
    [snapshot, selectedSiteId],
  );

  useEffect(() => {
    setMessage(null);
  }, [selectedEntry?.site.id, selectedEntry?.credential?.site_category]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void authController.subscribeSessionState((payload) => {
      if (cancelled) return;
      setSession(payload);
    }).then((dispose) => {
      if (cancelled) {
        dispose();
        return;
      }
      unlisten = dispose;
      return authController.getSessionState().then((payload) => {
        if (!cancelled) setSession(payload);
      });
    }).catch((err) => {
      console.error('Failed to subscribe to auth session state', err);
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
      // Authentication belongs to Electron, not this React mount. HMR and
      // closing the Accounts view must not cancel a login already in progress.
    };
  }, []);

  useEffect(() => registerAuthWorkspaceRefresh(() => { void refresh(); }), [refresh]);

  useEffect(() => {
    if (session.status !== 'completed') return;
    void refresh().then(() => authController.cancelSession()).then(() => {
      setMessage('Login saved in the system credential store.');
      setSession(IDLE_SESSION);
    }).catch((err) => {
      setMessage(err instanceof Error ? err.message : String(err));
    });
  }, [refresh, session.status]);

  async function startLogin() {
    if (!selectedEntry) return;
    setBusy(true);
    setMessage(null);
    try {
      const next = await authController.startSession(selectedEntry.site.id);
      setSession(next);
    } catch (err) {
      setMessage(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function cancelLogin() {
    await authController.cancelSession();
    setSession(IDLE_SESSION);
  }

  async function saveManualOnlyFans(input: OnlyFansManualAuthInput) {
    setBusy(true);
    setMessage(null);
    try {
      const next = await authController.saveManualOnlyFans(input);
      setSession(next);
    } catch (err) {
      setMessage(err instanceof Error ? err.message : String(err));
      throw err;
    } finally {
      setBusy(false);
    }
  }

  async function removeCredential() {
    if (!selectedEntry) return;
    setBusy(true);
    setMessage(null);
    try {
      await authController.deleteCredential(selectedEntry.site.credential_owner_site_id);
      await refresh();
      setMessage('Credential removed.');
    } catch (err) {
      setMessage(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  const handleSelectSite = (siteId: string) => {
    if (session.status !== 'idle' && session.site_category && session.site_category !== siteId) {
      void authController.cancelSession();
      setSession(IDLE_SESSION);
    }
    setSelectedSiteId(siteId);
    onSelectSite?.(siteId);
  };

  return (
    <div className={styles.root}>
      {!hideSidebar && (
        <AuthSitesSidebar
          sites={snapshot?.sites ?? []}
          selectedSiteId={selectedSiteId}
          onSelect={handleSelectSite}
        />
      )}
      <AuthSiteDetail
        entry={selectedEntry}
        session={session}
        busy={busy}
        message={message}
        onStartLogin={startLogin}
        onSaveManualOnlyFans={saveManualOnlyFans}
        onCancelLogin={cancelLogin}
        onRemoveCredential={removeCredential}
      />
    </div>
  );
}
