import type { AuthSiteSnapshot } from '../../../controllers/authController';
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

export function AuthSitesSidebar({
  sites,
  selectedSiteId,
  onSelect,
}: {
  sites: AuthSiteSnapshot[];
  selectedSiteId: string | null;
  onSelect: (siteId: string) => void;
}) {
  return (
    <aside className={styles.sidebar}>
      <div className={styles.sidebarHeader}>
        <div className={styles.titleWrap}>
          <div className={styles.title}>Auth</div>
          <div className={styles.subtitle}>Log in and monitor credential health across sites.</div>
        </div>
      </div>
      <div className={styles.sidebarBody}>
        <div className={styles.list}>
          {sites.map((entry) => {
            const tone = authTone(entry.health, Boolean(entry.credential), entry.issues.length > 0);
            return (
              <button
                key={entry.site.id}
                type="button"
                className={`${styles.siteRow} ${selectedSiteId === entry.site.id ? styles.siteRowSelected : ''}`.trim()}
                onClick={() => onSelect(entry.site.id)}
              >
                <div className={styles.rowTop}>
                  <div className={styles.siteName}>{entry.site.name}</div>
                  <span className={`${styles.statusBadge} ${badgeClass(tone)}`.trim()}>
                    {authStatusLabel(entry.health, Boolean(entry.credential), entry.site)}
                  </span>
                </div>
                <div className={styles.rowMeta}>
                  <span className={styles.muted}>{entry.site.domain}</span>
                  {entry.queryCount > 0 && (
                    <span className={styles.smallBadge}>{entry.queryCount} queries</span>
                  )}
                  {entry.issues.length > 0 && (
                    <span className={styles.smallBadge}>{entry.issues.length} issues</span>
                  )}
                </div>
                <div className={styles.muted}>
                  {entry.health?.last_checked_at
                    ? `Last checked ${formatRelativeTime(entry.health.last_checked_at)}`
                    : entry.credential
                      ? 'Saved but not checked yet'
                      : entry.site.auth_required_for_full_access
                        ? 'Login recommended for full access'
                        : 'Login optional'}
                </div>
              </button>
            );
          })}
        </div>
      </div>
    </aside>
  );
}
