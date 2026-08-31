import { useEffect, useState, type FormEvent } from 'react';
import type { AuthSessionState, OnlyFansManualAuthInput } from '../../../shared/types/subscriptions';
import type { AuthSiteSnapshot } from '../../../shared/types/subscriptionsWorkspace';
import { GlassInput } from '../../../shared/ui/GlassInput/GlassInput';
import { formatRelativeTime, getAuthAccountStatus } from '../authUtils';
import { SiteIcon } from './SiteIcon';
import styles from '../AuthWorkspace.module.css';
import { t } from '../../../i18n';

export function AuthSiteDetail({
  entry,
  session,
  busy,
  message,
  onStartLogin,
  onSaveManualOnlyFans,
  onCancelLogin,
  onRemoveCredential,
}: {
  entry: AuthSiteSnapshot | null;
  session: AuthSessionState;
  busy: boolean;
  message: string | null;
  onStartLogin: () => Promise<void>;
  onSaveManualOnlyFans: (input: OnlyFansManualAuthInput) => Promise<void>;
  onCancelLogin: () => Promise<void>;
  onRemoveCredential: () => Promise<void>;
}) {
  const [manualOpen, setManualOpen] = useState(false);
  const [cookie, setCookie] = useState('');
  const [userAgent, setUserAgent] = useState('');
  const [xBc, setXBc] = useState('');

  useEffect(() => {
    setManualOpen(false);
    setCookie('');
    setUserAgent('');
    setXBc('');
  }, [entry?.site.id]);

  if (!entry) {
    return (
      <main className={styles.detail}>
        <div className={styles.emptyState}>
          <div className={styles.sectionTitle}>{t("Select a site")}</div>
          <div className={styles.muted}>{t("Authentication is managed per site here, not per subscription.")}</div>
        </div>
      </main>
    );
  }

  const sessionActive = session.site_category === entry.site.credential_owner_site_id && (
    session.status === 'starting' || session.status === 'active' || session.status === 'loading'
  );
  const hasEmbeddedSession = session.site_category === entry.site.credential_owner_site_id && (
    session.status === 'starting' || session.status === 'active' || session.status === 'loading' || session.status === 'error'
  );
  const concreteIssue = entry.health?.last_error ?? entry.issues[0]?.message ?? '';
  const notice = concreteIssue || message || '';
  const accountStatus = getAuthAccountStatus(entry);
  const accountState = accountStatus.label;
  const usageLabel = entry.queryCount === 0
    ? 'Not used by a subscription'
    : `${entry.queryCount} ${entry.queryCount === 1 ? 'query' : 'queries'}`;
  const loginDescription = entry.site.id === 'pixiv'
    ? 'Sign in on Pixiv. Picto captures the OAuth session when the site completes authentication.'
    : entry.site.id === 'ehentai'
      ? 'Sign in with your E-Hentai account. Picto verifies ExHentai access before saving the browser session.'
    : entry.site.credential_types.includes('api_key')
      ? `Sign in on ${entry.site.name}. Picto retrieves and stores the account API credential.`
      : `Sign in on ${entry.site.name}. Picto stores the resulting browser session securely.`;
  const isOnlyFans = entry.site.id === 'onlyfans';

  async function submitManualOnlyFans(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    await onSaveManualOnlyFans({ cookie, user_agent: userAgent, x_bc: xBc });
    setCookie('');
    setUserAgent('');
    setXBc('');
    setManualOpen(false);
  }

  return (
    <main className={styles.detail}>
      <header className={styles.detailHeader}>
          <span className={styles.detailMark} aria-hidden="true"><SiteIcon domain={entry.site.domain} size={22} /></span>
          <div className={styles.titleWrap}>
            <div className={styles.heroTitle}>{entry.site.name}</div>
            <div className={styles.subtitle}>{entry.site.domain}</div>
          </div>
          <span className={`${styles.detailStatus} ${styles[`detailStatus${accountStatus.tone === 'success' ? 'Success' : accountStatus.tone === 'attention' ? 'Attention' : 'Idle'}`]}`.trim()}>{accountState}</span>
      </header>

      <div className={styles.detailBody}>
        <section className={styles.accountPanel} aria-label={t("Account details")}>
          <dl className={styles.factGrid}>
            <dt>{t("Account")}</dt><dd>{accountState}</dd>
            <dt>{t("Credential")}</dt><dd>{entry.credential?.credential_type ?? 'Not configured'}</dd>
            <dt>{t("Subscriptions")}</dt><dd>{usageLabel}</dd>
            <dt>{t("Last checked")}</dt><dd>{entry.health?.last_checked_at ? formatRelativeTime(entry.health.last_checked_at) : t("Never")}</dd>
          </dl>

        <div className={styles.inlineActions}>
          <button type="button" className={styles.buttonPrimary} disabled={busy || sessionActive} onClick={() => { void onStartLogin(); }}>
            {sessionActive ? t("Logging in…") : entry.credential ? t("Refresh Login") : t("Log In")}
          </button>
          {sessionActive && (
            <button type="button" className={styles.buttonSecondary} disabled={busy} onClick={() => { void onCancelLogin(); }}>
              {t("Cancel")}</button>
          )}
          {entry.credential && (
            <button type="button" className={styles.buttonDanger} disabled={busy} onClick={() => { void onRemoveCredential(); }}>
              {t("Remove")}</button>
          )}
        </div>
          {isOnlyFans && (
            <div className={styles.manualAuth}>
              <button
                type="button"
                className={styles.manualToggle}
                disabled={busy || sessionActive}
                onClick={() => setManualOpen((open) => !open)}
              >
                {manualOpen ? t("Hide manual login") : t("Enter session manually…")}
              </button>
              {manualOpen && (
                <form className={styles.manualForm} onSubmit={(event) => { void submitManualOnlyFans(event); }}>
                  <label>
                    <span>{t("Cookie")}</span>
                    <GlassInput type="password" value={cookie} onChange={(event) => setCookie(event.target.value)} autoComplete="off" spellCheck={false} required />
                  </label>
                  <label>
                    <span>{t("User-Agent")}</span>
                    <GlassInput value={userAgent} onChange={(event) => setUserAgent(event.target.value)} autoComplete="off" spellCheck={false} required />
                  </label>
                  <label>
                    <span>{t("X-BC")}</span>
                    <GlassInput type="password" value={xBc} onChange={(event) => setXBc(event.target.value)} autoComplete="off" spellCheck={false} required />
                  </label>
                  <button type="submit" className={styles.buttonSecondary} disabled={busy}>{t("Save Session")}</button>
                </form>
              )}
            </div>
          )}
          <div className={styles.noticeSlot} data-visible={Boolean(notice)} aria-live="polite">
            {notice || '\u00a0'}
          </div>
        </section>

        <section className={styles.loginPanel}>
          <div className={styles.sectionTitle}>{t("Sign in")}</div>
          <p className={styles.helper}>{loginDescription}</p>
          <div className={styles.sessionSlot} data-visible={hasEmbeddedSession} aria-live="polite">
            {hasEmbeddedSession && (
              <>
                <span className={styles.sessionPulse} aria-hidden="true" />
                <span>{session.message ?? session.current_url ?? 'Waiting for the site…'}</span>
              </>
            )}
          </div>
        </section>

        <section className={styles.issuesPanel}>
          <div className={styles.sectionTitle}>{t("Runtime Issues")}</div>
          <div className={styles.issueList}>
            {entry.issues.length === 0 ? (
              <div className={styles.emptyIssue}>{t("No authentication issues")}</div>
            ) : entry.issues.map((issue) => (
              <article key={issue.issue_id} className={styles.issueCard}>
                <strong>{issue.message}</strong>
                {issue.detail && <span>{issue.detail}</span>}
                <span>{t("Seen ")}{formatRelativeTime(issue.last_seen_at)}</span>
              </article>
            ))}
          </div>
        </section>
      </div>
    </main>
  );
}
