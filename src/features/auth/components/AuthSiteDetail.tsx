import type { AuthSessionState } from '../../../shared/types/subscriptions';
import type { AuthSiteSnapshot } from '../../../shared/types/subscriptionsWorkspace';
import { authStatusLabel, authTone, formatRelativeTime, parseBooruApiCredential, parseCookies, supportsInlineAuth } from '../authUtils';
import styles from '../AuthWorkspace.module.css';

function badgeClass(tone: 'running' | 'paused' | 'attention' | 'idle'): string {
  return tone === 'running'
    ? styles.statusRunning
    : tone === 'paused'
      ? styles.statusPaused
      : tone === 'attention'
        ? styles.statusAttention
        : styles.statusIdle;
}

export type AuthManualFormState = {
  displayName: string;
  username: string;
  password: string;
  cookiesRaw: string;
  booruApiRaw: string;
};

export function AuthSiteDetail({
  entry,
  session,
  busy,
  message,
  manualForm,
  onManualFormChange,
  onStartLogin,
  onCancelLogin,
  onSaveManual,
  onRemoveCredential,
}: {
  entry: AuthSiteSnapshot | null;
  session: AuthSessionState;
  busy: boolean;
  message: string | null;
  manualForm: AuthManualFormState;
  onManualFormChange: (patch: Partial<AuthManualFormState>) => void;
  onStartLogin: () => Promise<void>;
  onCancelLogin: () => Promise<void>;
  onSaveManual: () => Promise<void>;
  onRemoveCredential: () => Promise<void>;
}) {
  if (!entry) {
    return (
      <main className={styles.content}>
        <div className={styles.emptyState}>
          <div className={styles.sectionTitle}>Select a site</div>
          <div className={styles.muted}>Authentication is managed per site here, not per subscription.</div>
        </div>
      </main>
    );
  }

  const tone = authTone(entry.health, Boolean(entry.credential), entry.issues.length > 0);
  const inlineAuth = supportsInlineAuth(entry.site.id);
  const sessionActive = session.site_category === entry.site.id && (
    session.status === 'starting' || session.status === 'active' || session.status === 'loading'
  );
  const hasEmbeddedSession = session.site_category === entry.site.id && (
    session.status === 'starting' || session.status === 'active' || session.status === 'loading' || session.status === 'error'
  );
  const usesEmbeddedOnly = inlineAuth;

  return (
    <main className={styles.content}>
      <section className={styles.hero}>
        <div className={styles.heroTop}>
          <div className={styles.titleWrap}>
            <div className={styles.heroTitle}>{entry.site.name}</div>
            <div className={styles.subtitle}>{entry.site.domain}</div>
          </div>
          <span className={`${styles.statusBadge} ${badgeClass(tone)}`.trim()}>
            {authStatusLabel(entry.health, Boolean(entry.credential), entry.site)}
          </span>
        </div>

        <div className={styles.detailMeta}>
          <span className={styles.smallBadge}>{entry.credential?.credential_type ?? 'not configured'}</span>
          {entry.queryCount > 0 && <span className={styles.smallBadge}>{entry.queryCount} queries use this site</span>}
          {entry.health?.last_checked_at && <span className={styles.smallBadge}>checked {formatRelativeTime(entry.health.last_checked_at)}</span>}
        </div>

        <div className={styles.muted}>
          {entry.site.auth_required_for_full_access
            ? 'This site commonly requires authentication for full access or stable subscription runs.'
            : 'Authentication is optional for this site, but storing it here keeps subscriptions predictable.'}
        </div>

        <div className={styles.inlineActions}>
          {inlineAuth && (
            <button type="button" className={styles.button} disabled={busy} onClick={() => { void onStartLogin(); }}>
              {sessionActive ? 'Logging in…' : entry.credential ? 'Refresh Login' : 'Log In'}
            </button>
          )}
          {sessionActive && (
            <button type="button" className={styles.buttonSecondary} disabled={busy} onClick={() => { void onCancelLogin(); }}>
              Cancel
            </button>
          )}
          {entry.credential && (
            <button type="button" className={styles.buttonDanger} disabled={busy} onClick={() => { void onRemoveCredential(); }}>
              Remove Credential
            </button>
          )}
        </div>
        {entry.health?.last_error && <div className={styles.errorBanner}>{entry.health.last_error}</div>}
        {message && <div className={styles.panel}>{message}</div>}
      </section>

      {inlineAuth && (
        <section className={styles.panel}>
          <div className={styles.sectionHeader}>
            <div className={styles.sectionTitle}>Site Login</div>
          </div>
          <div className={styles.muted}>
            {entry.site.id === 'pixiv'
              ? 'Sign in in the login window and Picto will capture the Pixiv OAuth code automatically.'
              : entry.site.id === 'gelbooru' || entry.site.id === 'rule34'
                ? 'Sign in in the login window and Picto will save the site user_id and api_key from account settings.'
                : entry.site.id === 'twitter'
                  ? 'Sign in in the login window and Picto will capture the authenticated cookies gallery-dl needs.'
                  : entry.site.id === 'furaffinity'
                    ? 'Sign in in the login window and Picto will capture the session cookies gallery-dl needs.'
                    : 'Sign in in the login window to continue.'}
          </div>
          <div className={styles.emptyState}>
            <div className={styles.sectionTitle}>{hasEmbeddedSession ? 'Login in progress' : 'No active login'}</div>
            <div className={styles.muted}>
              {hasEmbeddedSession
                ? 'A separate login window is open. Complete authentication there and Picto will save the credential here.'
                : 'Start login to open the site in a separate window.'}
            </div>
          </div>
          <div className={styles.muted}>
            {hasEmbeddedSession
              ? session.message ?? session.current_url ?? 'Waiting for the site session…'
              : 'The login flow opens in a separate window, not inside this pane.'}
          </div>
        </section>
      )}

      {!usesEmbeddedOnly && (
        <section className={styles.fieldCard}>
          <div className={styles.sectionHeader}>
            <div className={styles.sectionTitle}>Manual Credential</div>
          </div>

          <label className={styles.label}>
            Display Name
            <input
              className={styles.field}
              value={manualForm.displayName}
              onChange={(event) => onManualFormChange({ displayName: event.target.value })}
            />
          </label>

          {entry.site.id === 'rule34' ? (
            <label className={styles.label}>
              API Credential String
              <input
                className={styles.field}
                placeholder="&api_key=YOUR_API_KEY&user_id=YOUR_USER_ID"
                value={manualForm.booruApiRaw}
                onChange={(event) => onManualFormChange({ booruApiRaw: event.target.value })}
              />
            </label>
          ) : (entry.site.id === 'twitter' || entry.site.id === 'furaffinity') ? (
            <label className={styles.label}>
              Cookies
              <textarea
                className={styles.textarea}
                value={manualForm.cookiesRaw}
                onChange={(event) => onManualFormChange({ cookiesRaw: event.target.value })}
              />
            </label>
          ) : (
            <div className={styles.fieldGrid}>
              <label className={styles.label}>
                Username
                <input
                  className={styles.field}
                  value={manualForm.username}
                  onChange={(event) => onManualFormChange({ username: event.target.value })}
                />
              </label>
              <label className={styles.label}>
                Password / Key
                <input
                  type="password"
                  className={styles.field}
                  value={manualForm.password}
                  onChange={(event) => onManualFormChange({ password: event.target.value })}
                />
              </label>
            </div>
          )}

          <div className={styles.inlineActions}>
            <button type="button" className={styles.buttonSecondary} disabled={busy} onClick={() => { void onSaveManual(); }}>
              Save Credential
            </button>
          </div>
          {entry.site.id === 'rule34' && manualForm.booruApiRaw.trim() && !parseBooruApiCredential(manualForm.booruApiRaw) && (
            <div className={styles.muted}>Paste a string containing both `api_key` and `user_id`.</div>
          )}
          {(entry.site.id === 'twitter' || entry.site.id === 'furaffinity') && manualForm.cookiesRaw.trim() && Object.keys(parseCookies(manualForm.cookiesRaw)).length === 0 && (
            <div className={styles.muted}>Use `name=value` cookie lines separated by newlines or semicolons.</div>
          )}
        </section>
      )}

      <section className={styles.panel}>
        <div className={styles.sectionHeader}>
          <div className={styles.sectionTitle}>Runtime Issues</div>
        </div>
        {entry.issues.length === 0 ? (
          <div className={styles.emptyState}>
            <div className={styles.sectionTitle}>No auth issues</div>
            <div className={styles.muted}>Credential and session-related subscription failures will surface here.</div>
          </div>
        ) : (
          entry.issues.map((issue) => (
            <div key={issue.issue_id} className={styles.issueCard}>
              <div className={styles.sectionTitle}>{issue.issue_kind}</div>
              <div>{issue.message}</div>
              {issue.detail && <div className={styles.muted}>{issue.detail}</div>}
              <div className={styles.muted}>Seen {formatRelativeTime(issue.last_seen_at)}</div>
            </div>
          ))
        )}
      </section>
    </main>
  );
}
