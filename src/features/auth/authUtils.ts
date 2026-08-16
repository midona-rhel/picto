import type { CredentialHealth, SubscriptionSiteInfo } from '../../shared/types/subscriptions';

export function formatRelativeTime(value: string | null | undefined): string {
  if (!value) return 'Never';
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

export function authTone(health: CredentialHealth | null, hasCredential: boolean, hasIssues: boolean): 'running' | 'paused' | 'attention' | 'idle' {
  if (!hasCredential && hasIssues) return 'attention';
  const status = health?.health_status ?? '';
  if (status === 'expired' || status === 'unauthorized' || status === 'error' || status === 'missing') return 'attention';
  if (status === 'healthy' || status === 'valid') return 'running';
  if (hasCredential) return 'paused';
  return 'idle';
}

export function authStatusLabel(health: CredentialHealth | null, hasCredential: boolean, site: SubscriptionSiteInfo): string {
  if (!hasCredential) return site.auth_required_for_full_access ? 'Missing' : 'Optional';
  const status = health?.health_status ?? 'saved';
  if (status === 'valid') return 'Healthy';
  return status.charAt(0).toUpperCase() + status.slice(1);
}
