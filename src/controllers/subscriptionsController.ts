import * as api from '../platform/api';
import type {
  CredentialDomain,
  CredentialHealth,
  CredentialType,
  PixivOAuthExchangeResult,
  PixivOAuthStartResult,
  FailedPostGroup,
  SubscriptionInfo,
  SubscriptionIssueRecord,
  SubscriptionProgressEvent,
  SubscriptionRunRecord,
  SubscriptionSiteInfo,
} from '../shared/types/subscriptions';
import { groupFailedPostAttempts } from '../features/subscriptions/subscriptionUtils';

export interface SubscriptionListMetrics {
  failedPostCount: number;
  openIssueCount: number;
  lastActivityAt: string | null;
}

export interface SubscriptionWorkspaceSnapshot {
  subscriptions: SubscriptionInfo[];
  sites: SubscriptionSiteInfo[];
  credentials: CredentialDomain[];
  credentialHealth: CredentialHealth[];
  runningSubscriptionIds: string[];
  runningProgress: SubscriptionProgressEvent[];
  listMetrics: Record<string, SubscriptionListMetrics>;
}

function deriveLastActivityAt(subscription: SubscriptionInfo): string | null {
  let latest: string | null = null;
  for (const query of subscription.queries) {
    if (!query.last_check_time) continue;
    if (!latest || query.last_check_time > latest) latest = query.last_check_time;
  }
  return latest;
}

export const subscriptionsController = {
  async loadWorkspaceSnapshot(): Promise<SubscriptionWorkspaceSnapshot> {
    const [subscriptions, sites, credentials, credentialHealth, runningSubscriptionIds, runningProgress] = await Promise.all([
      api.getSubscriptions(),
      api.getSubscriptionSites(),
      api.listCredentials(),
      api.listCredentialHealth(),
      api.getRunningSubscriptions(),
      api.getRunningSubscriptionProgress(),
    ]);

    const metricsEntries = await Promise.all(
      subscriptions.map(async (subscription) => {
        const [issues, attempts] = await Promise.all([
          api.listSubscriptionIssues(subscription.id, null, 100),
          api.listSubscriptionDownloadAttempts(subscription.id, null, 100),
        ]);
        const failedPosts = groupFailedPostAttempts(attempts, subscription.queries);
        const openIssueCount = issues.filter((issue) => issue.status !== 'resolved').length;
        return [
          subscription.id,
          {
            failedPostCount: failedPosts.length,
            openIssueCount,
            lastActivityAt: deriveLastActivityAt(subscription),
          },
        ] as const;
      }),
    );

    return {
      subscriptions,
      sites,
      credentials,
      credentialHealth,
      runningSubscriptionIds,
      runningProgress,
      listMetrics: Object.fromEntries(metricsEntries),
    };
  },

  async refreshRuntimeState(): Promise<{
    runningSubscriptionIds: string[];
    runningProgress: SubscriptionProgressEvent[];
  }> {
    const [runningSubscriptionIds, runningProgress] = await Promise.all([
      api.getRunningSubscriptions(),
      api.getRunningSubscriptionProgress(),
    ]);
    return { runningSubscriptionIds, runningProgress };
  },

  async listRuns(subscriptionId: string): Promise<SubscriptionRunRecord[]> {
    return api.listSubscriptionRuns(subscriptionId, 20);
  },

  async listIssues(subscriptionId: string): Promise<SubscriptionIssueRecord[]> {
    return api.listSubscriptionIssues(subscriptionId, null, 50);
  },

  async listFailedPosts(subscription: SubscriptionInfo): Promise<FailedPostGroup[]> {
    const attempts = await api.listSubscriptionDownloadAttempts(subscription.id, null, 100);
    return groupFailedPostAttempts(attempts, subscription.queries);
  },

  getSites(): Promise<SubscriptionSiteInfo[]> {
    return api.getSubscriptionSites();
  },

  getSubscriptions(): Promise<SubscriptionInfo[]> {
    return api.getSubscriptions();
  },

  create(input: {
    name: string;
    initial_post_limit?: number | null;
    periodic_post_limit?: number | null;
  }): Promise<SubscriptionInfo> {
    return api.createSubscription(input);
  },

  createGroup(name: string, schedule?: string | null): Promise<unknown> {
    return api.createGroup(name, schedule);
  },

  rename(id: string, name: string): Promise<void> {
    return api.renameSubscription(id, name);
  },

  delete(id: string): Promise<void> {
    return api.deleteSubscription(id);
  },

  pause(id: string, paused: boolean): Promise<void> {
    return api.pauseSubscription(id, paused);
  },

  reset(id: string): Promise<void> {
    return api.resetSubscription(id);
  },

  run(id: string): Promise<void> {
    return api.runSubscription(id);
  },

  stop(id: string): Promise<void> {
    return api.stopSubscription(id);
  },

  setAutoCollections(id: string, autoCollections: boolean): Promise<void> {
    return api.setSubscriptionAutoCollections(id, autoCollections);
  },

  addQuery(
    subscriptionId: string,
    siteId: string,
    queryText: string,
    notes?: string | null,
  ): Promise<SubscriptionInfo['queries'][number]> {
    return api.addSubscriptionQuery(subscriptionId, siteId, queryText, notes);
  },

  editQuery(
    id: number,
    siteId: string,
    queryText: string,
    displayName?: string | null,
    notes?: string | null,
  ): Promise<void> {
    return api.editSubscriptionQuery(id, siteId, queryText, displayName, notes);
  },

  deleteQuery(id: string): Promise<void> {
    return api.deleteSubscriptionQuery(id);
  },

  pauseQuery(id: string, paused: boolean): Promise<void> {
    return api.pauseSubscriptionQuery(id, paused);
  },

  resetQuery(id: string): Promise<void> {
    return api.resetSubscriptionQuery(id);
  },

  runQuery(subscriptionId: string, queryId: string): Promise<void> {
    return api.runSubscriptionQuery(subscriptionId, queryId);
  },

  stopQuery(subscriptionId: string, queryId: string): Promise<void> {
    return api.stopSubscriptionQuery(subscriptionId, queryId);
  },

  retryFailedPost(input: {
    subscription_id: string;
    query_id: string;
    site_id: string;
    post_id: string;
  }): Promise<void> {
    return api.retrySubscriptionFailedPost(input);
  },

  listCredentials(): Promise<CredentialDomain[]> {
    return api.listCredentials();
  },

  listCredentialHealth(): Promise<CredentialHealth[]> {
    return api.listCredentialHealth();
  },

  setCredential(input: {
    site_category: string;
    credential_type: CredentialType;
    display_name?: string | null;
    username?: string | null;
    password?: string | null;
    cookies?: Record<string, string> | null;
    oauth_token?: string | null;
  }): Promise<void> {
    return api.setCredential(input);
  },

  deleteCredential(siteCategory: string): Promise<void> {
    return api.deleteCredential(siteCategory);
  },

  pixivOAuthStart(): Promise<PixivOAuthStartResult> {
    return api.pixivOAuthStart();
  },

  pixivOAuthExchange(code: string, codeVerifier: string, phpsessid?: string | null): Promise<PixivOAuthExchangeResult> {
    return api.pixivOAuthExchange(code, codeVerifier, phpsessid);
  },

  openExternalUrl(url: string): Promise<void> {
    return api.openExternalUrl(url);
  },
};
