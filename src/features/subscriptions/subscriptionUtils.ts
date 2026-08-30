import type {
  CredentialDomain,
  CredentialHealth,
  FailedPostGroup,
  SubscriptionProgressEvent,
  SubscriptionInfo,
  SubscriptionQueryInfo,
  SubscriptionSiteInfo,
} from '../../shared/types/subscriptions';

const DEFAULT_SOURCE_POST_BATCH_SIZE = 100;

export function isGalleryImportJob(subscription: SubscriptionInfo): boolean {
  return subscription.queries?.length === 1 && subscription.queries[0]?.site_id === 'ehentai';
}

export function isVisibleGalleryImportJob(subscription: SubscriptionInfo): boolean {
  return isGalleryImportJob(subscription) && subscription.run_status != null;
}

export function getSubscriptionRunTarget(
  subscription: SubscriptionInfo,
  mode?: string | null,
): number {
  const perQuery = subscription.posts_per_run || DEFAULT_SOURCE_POST_BATCH_SIZE;
  if (mode === 'manual-query') return perQuery;
  const activeQueries = subscription.queries.filter((query) => !query.paused).length;
  return perQuery * Math.max(1, activeQueries);
}

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
  running?: boolean;
  progress?: SubscriptionProgressEvent | null;
  failedPostCount: number;
  openIssueCount: number;
}): 'running' | 'paused' | 'attention' | 'idle' {
  if (input.running) return 'running';
  if (input.paused) return 'paused';
  if (input.progress) return 'running';
  if (input.failedPostCount > 0 || input.openIssueCount > 0) return 'attention';
  return 'idle';
}

export function isQueryCompleted(query: SubscriptionQueryInfo): boolean {
  return !query.paused
    && query.completed_initial_run
    && query.successful_run_count >= 1
    && query.last_success_at != null
    && query.last_failure_kind == null;
}

export function isSubscriptionCompleted(subscription: SubscriptionInfo): boolean {
  return !subscription.paused
    && subscription.queries.length > 0
    && subscription.queries.every((query) => isQueryCompleted(query));
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
  if (!site) {
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
  const blocking = broken || (site.auth_strictly_required && missing);

  if (blocking) {
    return {
      tone: 'attention',
      label: missing ? 'Sign in required' : health?.last_error?.trim() || 'Sign in again',
      blocking: true,
    };
  }
  if (credential && (healthStatus === 'valid' || healthStatus === 'healthy')) {
    return { tone: 'running', label: 'Auth ok', blocking: false };
  }
  if (credential) {
    return { tone: 'paused', label: 'Auth saved', blocking: false };
  }
  if (site.auth_required_for_full_access) {
    return { tone: 'idle', label: '', blocking: false };
  }
  return { tone: 'idle', label: '', blocking: false };
}

/** Human-readable, actionable text for a query's recorded failure kind. */
export function describeFailure(kind: string | null, message: string | null): string | null {
  switch (kind) {
    case 'not_found': return 'User/query not found — check the handle';
    case 'unauthorized': return 'Login rejected — check the account in Accounts';
    case 'expired': return 'Session expired — log in again in Accounts';
    case 'rate_limited': return 'Rate limited by the site — resumes automatically when the limit resets';
    case 'network': return 'Network error — check your connection';
    case 'environment': return 'Local setup problem — check the logs';
    case 'stale': return 'Interrupted by app shutdown';
    case 'inbox_full': return 'Paused — inbox is full';
    default: return message;
  }
}
