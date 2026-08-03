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

export function parseCookies(raw: string): Record<string, string> {
  const out: Record<string, string> = {};
  const entries = raw.split(/[\n;]/).map((value) => value.trim()).filter(Boolean);
  for (const entry of entries) {
    const idx = entry.indexOf('=');
    if (idx <= 0) continue;
    const key = entry.slice(0, idx).trim();
    const value = entry.slice(idx + 1).trim();
    if (!key || !value) continue;
    out[key] = value;
  }
  return out;
}

export function parseBooruApiCredential(raw: string): { userId: string; apiKey: string } | null {
  let query = raw.trim();
  if (!query) return null;
  const qIndex = query.indexOf('?');
  if (qIndex >= 0) query = query.slice(qIndex + 1);
  if (query.startsWith('&') || query.startsWith('?')) query = query.slice(1);
  const params = new URLSearchParams(query);
  const userId = (params.get('user_id') ?? params.get('user-id') ?? '').trim();
  const apiKey = (params.get('api_key') ?? params.get('api-key') ?? '').trim();
  return userId && apiKey ? { userId, apiKey } : null;
}

const INLINE_AUTH_SITES = new Set([
  'pixiv',
  'gelbooru',
  'rule34',
  'twitter',
  'furaffinity',
  // Cookie-capture login windows (mirror COOKIE_LOGIN_SITES in
  // electron/windows/windowManager.mjs):
  'patreon',
  'fanbox',
  'fantia',
  'instagram',
  'deviantart',
  'nijie',
]);

export function supportsInlineAuth(siteId: string): boolean {
  return INLINE_AUTH_SITES.has(siteId);
}

export function requiresCookiePair(siteId: string): boolean {
  return siteId === 'twitter' || siteId === 'furaffinity';
}
