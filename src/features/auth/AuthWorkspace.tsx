import { useCallback, useEffect, useMemo, useState } from 'react';
import { authController } from '../../controllers/authController';
import { registerAuthWorkspaceRefresh } from '../../runtime/subscriptionsSettle';
import type { AuthSessionState } from '../../shared/types/subscriptions';
import type { AuthSiteSnapshot, AuthWorkspaceSnapshot } from '../../shared/types/subscriptionsWorkspace';
import { AuthSiteDetail, type AuthManualFormState } from './components/AuthSiteDetail';
import { AuthSitesSidebar } from './components/AuthSitesSidebar';
import { parseBooruApiCredential, parseCookies } from './authUtils';
import styles from './AuthWorkspace.module.css';

const IDLE_SESSION: AuthSessionState = {
  site_category: null,
  status: 'idle',
  title: null,
  current_url: null,
  message: null,
  credential: null,
};

function emptyManualForm(entry: AuthSiteSnapshot | null): AuthManualFormState {
  return {
    displayName: entry?.credential?.display_name ?? entry?.site.name ?? '',
    username: '',
    password: '',
    cookiesRaw: '',
    booruApiRaw: '',
  };
}

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
  const [manualForm, setManualForm] = useState<AuthManualFormState>(emptyManualForm(null));

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
    setManualForm(emptyManualForm(selectedEntry));
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
    }).catch((err) => {
      console.error('Failed to subscribe to auth session state', err);
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
      void authController.cancelSession();
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

    if ((credential.site_category === 'gelbooru' || credential.credential_type === 'api_key') && credential.username && credential.password) {
      setBusy(true);
      void authController.setCredential({
        site_category: credential.site_category,
        credential_type: 'api_key',
        display_name: selectedEntry?.site.name ?? credential.site_category,
        username: credential.username,
        password: credential.password,
      }).then(async () => {
        await refresh();
        setMessage('Credential saved from the login window.');
      }).catch((err) => {
        setMessage(err instanceof Error ? err.message : String(err));
      }).finally(() => {
        setBusy(false);
        setSession(IDLE_SESSION);
      });
      return;
    }

    if (credential.credential_type === 'cookies' && credential.cookies) {
      setBusy(true);
      void authController.setCredential({
        site_category: credential.site_category,
        credential_type: 'cookies',
        display_name: selectedEntry?.site.name ?? credential.site_category,
        cookies: credential.cookies,
        expires_at: credential.expires_at ?? null,
      }).then(async () => {
        await refresh();
        setMessage('Credential saved from the login window.');
      }).catch((err) => {
        setMessage(err instanceof Error ? err.message : String(err));
      }).finally(() => {
        setBusy(false);
        setSession(IDLE_SESSION);
      });
    }
  }, [pixivVerifier, selectedEntry?.site.name, session]);

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

  async function saveManual() {
    if (!selectedEntry) return;
    setBusy(true);
    setMessage(null);
    try {
      if (selectedEntry.site.id === 'rule34') {
        const parsed = parseBooruApiCredential(manualForm.booruApiRaw);
        if (!parsed) throw new Error('Paste a credential string containing both api_key and user_id.');
        await authController.setCredential({
          site_category: selectedEntry.site.id,
          credential_type: 'api_key',
          display_name: manualForm.displayName || null,
          username: parsed.userId,
          password: parsed.apiKey,
        });
      } else if (selectedEntry.site.id === 'twitter') {
        const cookies = parseCookies(manualForm.cookiesRaw);
        if (!cookies.auth_token || !cookies.ct0) throw new Error('Twitter/X requires auth_token and ct0 cookies.');
        await authController.setCredential({
          site_category: selectedEntry.site.id,
          credential_type: 'cookies',
          display_name: manualForm.displayName || null,
          cookies,
        });
      } else if (selectedEntry.site.id === 'furaffinity') {
        const cookies = parseCookies(manualForm.cookiesRaw);
        if (!cookies.a || !cookies.b) throw new Error('FurAffinity requires a and b cookies.');
        await authController.setCredential({
          site_category: selectedEntry.site.id,
          credential_type: 'cookies',
          display_name: manualForm.displayName || null,
          cookies,
        });
      } else {
        if (!manualForm.password.trim()) throw new Error('Password or key value is required.');
        await authController.setCredential({
          site_category: selectedEntry.site.id,
          credential_type: 'username_password',
          display_name: manualForm.displayName || null,
          username: manualForm.username.trim() || null,
          password: manualForm.password.trim(),
        });
      }
      await refresh();
      setMessage('Credential saved.');
    } catch (err) {
      setMessage(err instanceof Error ? err.message : String(err));
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
        manualForm={manualForm}
        onManualFormChange={(patch) => setManualForm((current) => ({ ...current, ...patch }))}
        onStartLogin={startLogin}
        onCancelLogin={cancelLogin}
        onSaveManual={saveManual}
        onRemoveCredential={removeCredential}
      />
    </div>
  );
}
