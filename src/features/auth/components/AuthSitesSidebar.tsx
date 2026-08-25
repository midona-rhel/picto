import type { AuthSiteSnapshot } from '../../../shared/types/subscriptionsWorkspace';
import styles from '../AuthWorkspace.module.css';

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
        <span>Services</span>
        <span className={styles.sidebarCount}>{sites.length}</span>
      </div>
      <div className={styles.sidebarBody}>
        <div className={styles.list}>
          {sites.map((entry) => (
              <button
                key={entry.site.id}
                type="button"
                className={`${styles.siteRow} ${selectedSiteId === entry.site.id ? styles.siteRowSelected : ''}`.trim()}
                onClick={() => onSelect(entry.site.id)}
              >
                <span className={styles.siteMark} aria-hidden="true">{entry.site.name.slice(0, 1)}</span>
                <span className={styles.siteName}>{entry.site.name}</span>
                {entry.credential && <span className={styles.accountDot} aria-label="Signed in" />}
              </button>
          ))}
        </div>
      </div>
    </aside>
  );
}
