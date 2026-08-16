import { useEffect, useState } from 'react';
import type { AuthSessionState } from '../../../shared/types/subscriptions';
import type { AuthSiteSnapshot } from '../../../shared/types/subscriptionsWorkspace';
import { authStatusLabel, authTone, formatRelativeTime } from '../authUtils';
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

export function AuthSiteDetail({
  entry,
  session,
  busy,
  message,
  onStartLogin,
  onCancelLogin,
  onRemoveCredential,
  onSaveUsernamePassword,
}: {
  entry: AuthSiteSnapshot | null;
  session: AuthSessionState;
  busy: boolean;
  message: string | null;
  onStartLogin: () => Promise<void>;
  onCancelLogin: () => Promise<void>;
  onRemoveCredential: () => Promise<void>;
  onSaveUsernamePassword: (username: string, password: string) => Promise<void>;
}) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');

  useEffect(() => {
    setUsername('');
    setPassword('');
  }, [entry?.site.id]);

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
  const sessionActive = session.site_category === entry.site.credential_owner_site_id && (
    session.status === 'starting' || session.status === 'active' || session.status === 'loading'
  );
  const hasEmbeddedSession = session.site_category === entry.site.credential_owner_site_id && (
    session.status === 'starting' || session.status === 'active' || session.status === 'loading' || session.status === 'error'
  );
  const usesUsernamePassword = entry.site.manual_credential_types.includes('username_password');

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
          {entry.queryCount > 0 && <span className={styles.smallBadge}>{entry.queryCount} {entry.queryCount === 1 ? 'query uses' : 'queries use'} this site</span>}
          {entry.health?.last_checked_at && <span className={styles.smallBadge}>checked {formatRelativeTime(entry.health.last_checked_at)}</span>}
        </div>

        <div className={styles.muted}>
          {usesUsernamePassword
            ? 'Store this account in the system credential store. Picto passes it only to gallery-dl for this source.'
            : entry.site.auth_required_for_full_access
            ? 'This site commonly requires authentication for full access or stable subscription runs.'
            : 'Authentication is optional for this site, but storing it here keeps subscriptions predictable.'}
        </div>

        <div className={styles.inlineActions}>
          {!usesUsernamePassword && (
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

      <section className={styles.panel}>
          <div className={styles.sectionHeader}>
            <div className={styles.sectionTitle}>Site Login</div>
          </div>
          {usesUsernamePassword ? (
            <form
              className={styles.fieldCard}
              onSubmit={(event) => {
                event.preventDefault();
                void onSaveUsernamePassword(username, password).then(() => setPassword(''));
              }}
            >
              <div className={styles.fieldGrid}>
                <label className={styles.label}>
                  Username
                  <input
                    className={styles.field}
                    autoComplete="username"
                    value={username}
                    onChange={(event) => setUsername(event.target.value)}
                  />
                </label>
                <label className={styles.label}>
                  Password
                  <input
                    className={styles.field}
                    type="password"
                    autoComplete="current-password"
                    value={password}
                    onChange={(event) => setPassword(event.target.value)}
                  />
                </label>
              </div>
              <div className={styles.inlineActions}>
                <button
                  type="submit"
                  className={styles.button}
                  disabled={busy || !username.trim() || !password}
                >
                  {entry.credential ? 'Replace Credential' : 'Save Credential'}
                </button>
              </div>
            </form>
          ) : (
            <>
              <div className={styles.muted}>
                {entry.site.id === 'pixiv'
                  ? 'Sign in in the login window and Picto will capture the Pixiv OAuth code automatically.'
                  : entry.site.manual_credential_types.includes('cookies')
                    ? `Sign in in the login window and Picto will save only the ${entry.site.name} session cookies required by gallery-dl.`
                    : `Sign in in the login window and Picto will save the ${entry.site.name} user_id and api_key from account settings.`}
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
            </>
          )}
      </section>

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
