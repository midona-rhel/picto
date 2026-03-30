import * as api from '../platform/api';
import type {
  AuthSessionBounds,
  AuthSessionState,
  CredentialDomain,
  CredentialHealth,
  SubscriptionInfo,
  SubscriptionIssueRecord,
  SubscriptionSiteInfo,
} from '../shared/types/subscriptions';

export interface AuthSiteSnapshot {
  site: SubscriptionSiteInfo;
  subscriptions: SubscriptionInfo[];
  queryCount: number;
  credential: CredentialDomain | null;
  health: CredentialHealth | null;
  issues: SubscriptionIssueRecord[];
}

export interface AuthWorkspaceSnapshot {
  sites: AuthSiteSnapshot[];
}

const AUTH_ISSUE_KINDS = new Set(['unauthorized', 'expired']);

function canonicalSiteCategory(siteId: string): string {
  return siteId === 'pixivuser' ? 'pixiv' : siteId;
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

export const authController = {
  async loadWorkspaceSnapshot(): Promise<AuthWorkspaceSnapshot> {
    const [sites, subscriptions, credentials, healthEntries] = await Promise.all([
      api.getSubscriptionSites(),
      api.getSubscriptions(),
      api.listCredentials(),
      api.listCredentialHealth(),
    ]);

    const authSites = sites.filter((site) => site.auth_supported);
    const issuesBySite = new Map<string, SubscriptionIssueRecord[]>();

    await Promise.all(
      subscriptions.map(async (subscription) => {
        const issues = await api.listSubscriptionIssues(subscription.id, null, 100);
        const authIssues = issues.filter(isAuthIssue);
        if (!authIssues.length) return;
        const siteKeys = new Set(
          subscription.queries.map((query) => canonicalSiteCategory(query.site_id)),
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
        const siteKey = canonicalSiteCategory(site.id);
        const matchingSubscriptions = subscriptions.filter((subscription) =>
          subscription.queries.some((query) => canonicalSiteCategory(query.site_id) === siteKey),
        );
        const queryCount = matchingSubscriptions.reduce(
          (count, subscription) =>
            count
            + subscription.queries.filter((query) => canonicalSiteCategory(query.site_id) === siteKey).length,
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
    return api.startAuthSession(siteCategory, startUrl);
  },

  setSessionBounds(bounds: AuthSessionBounds): Promise<void> {
    return api.setAuthSessionBounds(bounds);
  },

  cancelSession(): Promise<void> {
    return api.cancelAuthSession();
  },

  setCredential: api.setCredential,
  deleteCredential: api.deleteCredential,
  pixivOAuthStart: api.pixivOAuthStart,
  pixivOAuthExchange: api.pixivOAuthExchange,
};
