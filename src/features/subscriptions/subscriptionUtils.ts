import type {
  CredentialDomain,
  CredentialHealth,
  FailedPostGroup,
  SubscriptionProgressEvent,
  SubscriptionQueryInfo,
  SubscriptionSiteInfo,
} from '../../shared/types/subscriptions';

export function formatRelativeTime(value: string | null | undefined): string {
  if (!value) return 'Never';

  const parsed = Date.parse(value);
  if (Number.isNaN(parsed)) return value;

  const deltaMs = parsed - Date.now();
  const deltaMinutes = Math.round(deltaMs / 60000);
  const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });

  if (Math.abs(deltaMinutes) < 60) return formatter.format(deltaMinutes, 'minute');
  const deltaHours = Math.round(deltaMinutes / 60);
  if (Math.abs(deltaHours) < 48) return formatter.format(deltaHours, 'hour');
  const deltaDays = Math.round(deltaHours / 24);
  if (Math.abs(deltaDays) < 30) return formatter.format(deltaDays, 'day');
  const deltaMonths = Math.round(deltaDays / 30);
  return formatter.format(deltaMonths, 'month');
}

export function formatDateTime(value: string | null | undefined): string {
  if (!value) return 'None';
  const parsed = Date.parse(value);
  if (Number.isNaN(parsed)) return value;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(parsed));
}

export function getSiteLabel(siteId: string, sites: SubscriptionSiteInfo[]): string {
  return sites.find((site) => site.id === siteId)?.name ?? siteId;
}

export function getSubscriptionSiteSummary(
  queries: SubscriptionQueryInfo[],
  sites: SubscriptionSiteInfo[],
): string {
  const siteIds = Array.from(new Set(queries.map((query) => query.site_id)));
  if (siteIds.length === 0) return 'No sites';
  if (siteIds.length === 1) return getSiteLabel(siteIds[0], sites);
  if (siteIds.length === 2) {
    return siteIds.map((siteId) => getSiteLabel(siteId, sites)).join(' + ');
  }
  return `${siteIds.length} sites`;
}

export function describeSubscriptionState(input: {
  paused: boolean;
  progress?: SubscriptionProgressEvent | null;
  failedPostCount: number;
  openIssueCount: number;
}): 'running' | 'paused' | 'attention' | 'idle' {
  if (input.progress) return 'running';
  if (input.paused) return 'paused';
  if (input.failedPostCount > 0 || input.openIssueCount > 0) return 'attention';
  return 'idle';
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
  const input = raw.trim();
  if (!input) return null;

  let query = input;
  const qIndex = input.indexOf('?');
  if (qIndex >= 0 && qIndex < input.length - 1) {
    query = input.slice(qIndex + 1);
  }
  if (query.startsWith('&') || query.startsWith('?')) {
    query = query.slice(1);
  }

  const params = new URLSearchParams(query);
  const apiKey = (params.get('api_key') ?? params.get('api-key') ?? '').trim();
  const userId = (params.get('user_id') ?? params.get('user-id') ?? '').trim();
  if (!apiKey || !userId) return null;
  return { userId, apiKey };
}

export function isPixivCategory(siteCategory: string): boolean {
  const normalized = siteCategory.trim().toLowerCase();
  return normalized === 'pixiv' || normalized === 'pixivuser';
}

export function isTwitterCategory(siteCategory: string): boolean {
  const normalized = siteCategory.trim().toLowerCase();
  return normalized === 'twitter' || normalized === 'x.com';
}

export function isGelbooruCategory(siteCategory: string): boolean {
  return siteCategory.trim().toLowerCase() === 'gelbooru';
}

export function isRule34Category(siteCategory: string): boolean {
  const normalized = siteCategory.trim().toLowerCase();
  return normalized === 'rule34' || normalized === 'rule34xxx' || normalized === 'rule34.xxx';
}

export function isFuraffinityCategory(siteCategory: string): boolean {
  return siteCategory.trim().toLowerCase() === 'furaffinity';
}

export function isBooruApiKeyCategory(siteCategory: string): boolean {
  return isGelbooruCategory(siteCategory) || isRule34Category(siteCategory);
}

export function getQueryModeLabel(query: SubscriptionQueryInfo): string {
  return query.completed_initial_run ? 'front scan' : 'catch-up';
}

export function getQueryResumeSummary(query: SubscriptionQueryInfo): string {
  if (query.completed_initial_run) return 'Front scan from newest items';
  if (query.resume_cursor) return `Catch-up cursor ${query.resume_cursor}`;
  return 'Catch-up start';
}

export function getQueryFailedCount(queryId: string, failedPosts: FailedPostGroup[]): number {
  return failedPosts
    .filter((group) => group.queryId === queryId)
    .reduce((count, group) => count + group.failedMembers, 0);
}

export function getQueryAuthState(input: {
  query: SubscriptionQueryInfo;
  sites: SubscriptionSiteInfo[];
  credentials: CredentialDomain[];
  credentialHealth: CredentialHealth[];
}): {
  tone: 'running' | 'paused' | 'attention' | 'idle';
  label: string;
  blocking: boolean;
} {
  const site = input.sites.find((entry) => entry.id === input.query.site_id) ?? null;
  if (!site || !site.auth_supported) {
    return { tone: 'idle', label: 'No auth', blocking: false };
  }

  const credential = input.credentials.find(
    (entry) => entry.site_category === site.credential_owner_site_id,
  ) ?? null;
  const health = input.credentialHealth.find(
    (entry) => entry.site_category === site.credential_owner_site_id,
  ) ?? null;
  const healthStatus = (health?.health_status ?? '').toLowerCase();
  const missing = !credential;
  const broken = healthStatus === 'unauthorized' || healthStatus === 'expired' || healthStatus === 'error' || healthStatus === 'missing';
  const blocking = site.auth_required_for_full_access && (missing || broken);

  if (blocking) {
    return { tone: 'attention', label: missing ? 'Auth needed' : 'Auth broken', blocking: true };
  }
  if (credential && (healthStatus === 'valid' || healthStatus === 'healthy')) {
    return { tone: 'running', label: 'Auth ok', blocking: false };
  }
  if (credential) {
    return { tone: 'paused', label: 'Auth saved', blocking: false };
  }
  if (site.auth_required_for_full_access) {
    return { tone: 'attention', label: 'Auth recommended', blocking: false };
  }
  return { tone: 'idle', label: 'Optional auth', blocking: false };
}

/** Human-readable, actionable text for a query's recorded failure kind. */
export function describeFailure(kind: string | null, message: string | null): string | null {
  switch (kind) {
    case 'not_found': return 'User/query not found — check the handle';
    case 'unauthorized': return 'Login rejected — check the account in Accounts';
    case 'expired': return 'Session expired — log in again in Accounts';
    case 'rate_limited': return 'Rate limited by the site — try again later';
    case 'network': return 'Network error — check your connection';
    case 'environment': return 'Local setup problem — check the logs';
    case 'stale': return 'Interrupted by app shutdown';
    case 'inbox_full': return 'Paused — inbox is full';
    default: return message;
  }
}
