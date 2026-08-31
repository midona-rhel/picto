import type { AuthSiteSnapshot } from '../../shared/types/subscriptionsWorkspace';
import { t } from '../../i18n';

export function formatRelativeTime(value: string | null | undefined): string {
  if (!value) return t('Never');
  const parsed = Date.parse(value);
  if (Number.isNaN(parsed)) return value;
  const deltaMinutes = Math.round((parsed - Date.now()) / 60000);
  const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });
  if (Math.abs(deltaMinutes) < 60) return formatter.format(deltaMinutes, 'minute');
  const deltaHours = Math.round(deltaMinutes / 60);
  if (Math.abs(deltaHours) < 48) return formatter.format(deltaHours, 'hour');
  const deltaDays = Math.round(deltaHours / 24);
  return formatter.format(deltaDays, 'day');
}

export function getAuthAccountStatus(entry: AuthSiteSnapshot): {
  label: string;
  tone: 'success' | 'attention' | 'idle';
} {
  const health = entry.health?.health_status.toLowerCase() ?? '';
  const unhealthy = health === 'unauthorized'
    || health === 'expired'
    || health === 'error'
    || health === 'missing'
    || entry.issues.length > 0;

  if (entry.credential && unhealthy) return { label: t("Needs attention"), tone: 'attention' };
  if (entry.credential) return { label: t("Signed in"), tone: 'success' };
  return { label: t("Not signed in"), tone: 'idle' };
}
