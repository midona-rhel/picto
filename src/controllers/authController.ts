import {
  cancelAuthSession,
  deleteCredential,
  getSubscriptionSites,
  getSubscriptions,
  listCredentialHealth,
  listCredentials,
  listSubscriptionIssues,
  pixivOAuthExchange,
  pixivOAuthStart,
  setAuthSessionBounds,
  setCredential,
  startAuthSession,
} from '../platform/subscriptionApi';
import { listen } from '../platform/ipc';
import type {
  AuthSessionBounds,
  AuthSessionState,
  SubscriptionIssueRecord,
  SubscriptionSiteInfo,
} from '../shared/types/subscriptions';
import type { AuthSiteSnapshot, AuthWorkspaceSnapshot } from '../shared/types/subscriptionsWorkspace';

const AUTH_ISSUE_KINDS = new Set([
  'unauthorized',
  'expired',
  'credential_missing',
  'credential_blocked',
]);

function credentialOwnerForSite(siteId: string, sites: SubscriptionSiteInfo[]): string {
  return sites.find((site) => site.id === siteId)?.credential_owner_site_id ?? siteId;
}

function isAuthIssue(issue: SubscriptionIssueRecord): boolean {
  if (AUTH_ISSUE_KINDS.has(issue.issue_kind)) return true;
  const haystack = `${issue.message} ${issue.detail ?? ''}`.toLowerCase();
  return haystack.includes('credential') || haystack.includes('login') || haystack.includes('auth');
}

function healthRank(site: AuthSiteSnapshot): number {
  const status = site.health?.health_status ?? 'missing';
  if (!site.credential) return site.site.auth_required_for_full_access ? 0 : 3;
  if (status === 'unauthorized' || status === 'expired' || status === 'error' || status === 'missing') return 1;
  if (status === 'valid' || status === 'healthy') return 2;
  return 3;
}

async function listAllOpenIssues(subscriptionId: string): Promise<SubscriptionIssueRecord[]> {
  const items: SubscriptionIssueRecord[] = [];
  let cursor: number | null = null;
  do {
    const page = await listSubscriptionIssues(subscriptionId, null, 100, cursor);
    items.push(...page.items);
    cursor = page.next_cursor;
  } while (cursor != null);
  return items;
}

export const authController = {
  async loadWorkspaceSnapshot(): Promise<AuthWorkspaceSnapshot> {
    const [sites, subscriptions, credentials, healthEntries] = await Promise.all([
      getSubscriptionSites(),
      getSubscriptions(),
      listCredentials(),
      listCredentialHealth(),
    ]);

    const authSites = sites.filter(
      (site) => site.auth_supported && site.id === site.credential_owner_site_id,
    );
    const issuesBySite = new Map<string, SubscriptionIssueRecord[]>();

    await Promise.all(
      subscriptions.map(async (subscription) => {
        const issues = await listAllOpenIssues(subscription.id);
        const authIssues = issues.filter(isAuthIssue);
        if (!authIssues.length) return;
        const siteKeys = new Set(
          subscription.queries.map((query) => credentialOwnerForSite(query.site_id, sites)),
        );
        for (const siteKey of siteKeys) {
          const existing = issuesBySite.get(siteKey) ?? [];
          existing.push(...authIssues);
          issuesBySite.set(siteKey, existing);
        }
      }),
    );

    const sitesWithState = authSites
      .map((site) => {
        const siteKey = site.credential_owner_site_id;
        const matchingSubscriptions = subscriptions.filter((subscription) =>
          subscription.queries.some(
            (query) => credentialOwnerForSite(query.site_id, sites) === siteKey,
          ),
        );
        const queryCount = matchingSubscriptions.reduce(
          (count, subscription) =>
            count
            + subscription.queries.filter(
              (query) => credentialOwnerForSite(query.site_id, sites) === siteKey,
            ).length,
          0,
        );
        return {
          site,
          subscriptions: matchingSubscriptions,
          queryCount,
          credential: credentials.find((entry) => entry.site_category === siteKey) ?? null,
          health: healthEntries.find((entry) => entry.site_category === siteKey) ?? null,
          issues: issuesBySite.get(siteKey) ?? [],
        } satisfies AuthSiteSnapshot;
      })
      .sort((left, right) => {
        const leftUsed = left.queryCount > 0 ? 0 : 1;
        const rightUsed = right.queryCount > 0 ? 0 : 1;
        if (leftUsed !== rightUsed) return leftUsed - rightUsed;
        const leftRank = healthRank(left);
        const rightRank = healthRank(right);
        if (leftRank !== rightRank) return leftRank - rightRank;
        return left.site.name.localeCompare(right.site.name);
      });

    return { sites: sitesWithState };
  },

  startSession(siteCategory: string, startUrl?: string | null): Promise<AuthSessionState> {
    return startAuthSession(siteCategory, startUrl);
  },

  setSessionBounds(bounds: AuthSessionBounds): Promise<void> {
    return setAuthSessionBounds(bounds);
  },

  cancelSession(): Promise<void> {
    return cancelAuthSession();
  },

  subscribeSessionState(onState: (session: AuthSessionState) => void): Promise<() => void> {
    return listen<AuthSessionState>('auth:session-state', ({ payload }) => {
      onState(payload);
    });
  },

  setCredential,
  deleteCredential,
  pixivOAuthStart,
  pixivOAuthExchange,
};
