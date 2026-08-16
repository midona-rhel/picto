import { useCallback, useEffect, useMemo, useState } from 'react';
import { authController } from '../../controllers/authController';
import { registerAuthWorkspaceRefresh } from '../../runtime/subscriptionsSettle';
import type { AuthSessionState } from '../../shared/types/subscriptions';
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
  credential: null,
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
  const [pixivVerifier, setPixivVerifier] = useState<string | null>(null);

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
    if (session.status !== 'completed' || !session.credential) return;

    const credential = session.credential;
    if (credential.site_category === 'pixiv' && credential.oauth_code && pixivVerifier) {
      setBusy(true);
      void authController.pixivOAuthExchange(
        credential.oauth_code,
        pixivVerifier,
        credential.phpsessid ?? null,
      ).then(async () => {
        await refresh();
        await authController.cancelSession();
        setMessage('Pixiv login completed.');
      }).catch((err) => {
        setMessage(err instanceof Error ? err.message : String(err));
      }).finally(() => {
        setBusy(false);
        setPixivVerifier(null);
        setSession(IDLE_SESSION);
      });
      return;
    }

    const selectedCredentialOwner = selectedEntry?.site.credential_owner_site_id;
    const oauthToken = credential.oauth_token?.trim();
    const oauthTokenSecret = credential.password?.trim();
    const isApprovedOAuthPayload = Boolean(
      selectedCredentialOwner
      && credential.site_category === selectedCredentialOwner
      && (credential.site_category === 'baraag' || credential.site_category === 'tumblr')
      && credential.credential_type === 'oauth_token'
      && oauthToken
      && (credential.site_category !== 'tumblr' || oauthTokenSecret),
    );
    if (isApprovedOAuthPayload) {
      setBusy(true);
      void authController.setCredential({
        site_category: credential.site_category,
        credential_type: 'oauth_token',
        display_name: selectedEntry?.site.name ?? credential.site_category,
        oauth_token: oauthToken,
        password: credential.site_category === 'tumblr' ? oauthTokenSecret : undefined,
      }).then(async () => {
        await refresh();
        await authController.cancelSession();
        setMessage('OAuth credential saved in the system credential store.');
      }).catch((err) => {
        setMessage(err instanceof Error ? err.message : String(err));
      }).finally(() => {
        setBusy(false);
        setSession(IDLE_SESSION);
      });
      return;
    }

    const cookies = credential.cookies ?? null;
    const isApprovedCookiePayload = Boolean(
      selectedCredentialOwner
      && credential.site_category === selectedCredentialOwner
      && credential.credential_type === 'cookies'
      && cookies
      && Object.keys(cookies).length > 0,
    );
    if (isApprovedCookiePayload) {
      setBusy(true);
      void authController.setCredential({
        site_category: credential.site_category,
        credential_type: 'cookies',
        display_name: selectedEntry?.site.name ?? credential.site_category,
        cookies,
      }).then(async () => {
        await refresh();
        await authController.cancelSession();
        setMessage('Login session saved in the system credential store.');
      }).catch((err) => {
        setMessage(err instanceof Error ? err.message : String(err));
      }).finally(() => {
        setBusy(false);
        setSession(IDLE_SESSION);
      });
      return;
    }
    const username = credential.username?.trim();
    const password = credential.password?.trim();
    const isApprovedApiKeyPayload = Boolean(
      selectedCredentialOwner
      && credential.site_category === selectedCredentialOwner
      && credential.credential_type === 'api_key'
      && username
      && password,
    );
    if (isApprovedApiKeyPayload) {
      setBusy(true);
      void authController.setCredential({
        site_category: credential.site_category,
        credential_type: 'api_key',
        display_name: selectedEntry?.site.name ?? credential.site_category,
        username,
        password,
      }).then(async () => {
        await refresh();
        await authController.cancelSession();
        setMessage('Credential saved from the login window.');
      }).catch((err) => {
        setMessage(err instanceof Error ? err.message : String(err));
      }).finally(() => {
        setBusy(false);
        setSession(IDLE_SESSION);
      });
    }
  }, [pixivVerifier, selectedEntry?.site.credential_owner_site_id, selectedEntry?.site.name, session]);

  async function startLogin() {
    if (!selectedEntry) return;
    setBusy(true);
    setMessage(null);
    try {
      if (selectedEntry.site.id === 'pixiv') {
        const challenge = await authController.pixivOAuthStart();
        setPixivVerifier(challenge.code_verifier);
        const next = await authController.startSession(selectedEntry.site.id, challenge.login_url);
        setSession(next);
      } else {
        const next = await authController.startSession(selectedEntry.site.id, null);
        setSession(next);
      }
    } catch (err) {
      setMessage(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function cancelLogin() {
    await authController.cancelSession();
    setSession(IDLE_SESSION);
    setPixivVerifier(null);
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
        onCancelLogin={cancelLogin}
        onRemoveCredential={removeCredential}
      />
    </div>
  );
}
