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
}: {
  entry: AuthSiteSnapshot | null;
  session: AuthSessionState;
  busy: boolean;
  message: string | null;
  onStartLogin: () => Promise<void>;
  onCancelLogin: () => Promise<void>;
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
  const sessionActive = session.site_category === entry.site.credential_owner_site_id && (
    session.status === 'starting' || session.status === 'active' || session.status === 'loading'
  );
  const hasEmbeddedSession = session.site_category === entry.site.credential_owner_site_id && (
    session.status === 'starting' || session.status === 'active' || session.status === 'loading' || session.status === 'error'
  );

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
          {entry.site.auth_required_for_full_access
            ? 'This site commonly requires authentication for full access or stable subscription runs.'
            : 'Authentication is optional for this site, but storing it here keeps subscriptions predictable.'}
        </div>

        <div className={styles.inlineActions}>
          <button type="button" className={styles.button} disabled={busy} onClick={() => { void onStartLogin(); }}>
            {sessionActive ? 'Logging in…' : entry.credential ? 'Refresh Login' : 'Log In'}
          </button>
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
          <div className={styles.muted}>
            {entry.site.id === 'pixiv'
              ? 'Sign in on Pixiv and Picto will capture the OAuth session automatically.'
              : entry.site.credential_types.includes('api_key')
                ? `Sign in on ${entry.site.name}; Picto will retrieve the account API credential automatically.`
                : `Sign in on ${entry.site.name}; Picto will retain the resulting session for gallery-dl.`}
          </div>
          <div className={styles.emptyState}>
            <div className={styles.sectionTitle}>{hasEmbeddedSession ? 'Login in progress' : 'No active login'}</div>
            <div className={styles.muted}>
              {hasEmbeddedSession
                ? 'Complete authentication in the site window. Picto will save the resulting session automatically.'
                : 'Start login to open the real site in a separate window.'}
            </div>
          </div>
          <div className={styles.muted}>
            {hasEmbeddedSession
              ? session.message ?? session.current_url ?? 'Waiting for the site session…'
              : 'The login flow opens in a separate window, not inside this pane.'}
          </div>
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
