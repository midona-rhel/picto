import type { AuthSiteSnapshot } from '../../../shared/types/subscriptionsWorkspace';
import { getAuthAccountStatus } from '../authUtils';
import { SiteIcon } from './SiteIcon';
import styles from '../AuthWorkspace.module.css';
import { t } from '../../../i18n';

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
        <span>{t("Services")}</span>
        <span className={styles.sidebarCount}>{sites.length}</span>
      </div>
      <div className={styles.sidebarBody}>
        <div className={styles.list}>
          {sites.map((entry) => {
            const status = getAuthAccountStatus(entry);
            return (
              <button
                key={entry.site.id}
                type="button"
                className={`${styles.siteRow} ${selectedSiteId === entry.site.id ? styles.siteRowSelected : ''}`.trim()}
                onClick={() => onSelect(entry.site.id)}
              >
                <span className={styles.siteMark} aria-hidden="true"><SiteIcon domain={entry.site.domain} size={14} /></span>
                <span className={styles.siteName}>{entry.site.name}</span>
                <span
                  className={`${styles.accountDot} ${styles[`accountDot${status.tone === 'success' ? 'Success' : status.tone === 'attention' ? 'Attention' : 'Idle'}`]}`.trim()}
                  aria-label={status.label}
                />
              </button>
            );
          })}
        </div>
      </div>
    </aside>
  );
}
