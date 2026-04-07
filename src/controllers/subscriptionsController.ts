import { openExternalUrl } from '../platform/shellApi';
import {
  addSubscriptionQuery,
  createGroup,
  createSubscription,
  deleteCredential,
  deleteSubscription,
  deleteSubscriptionQuery,
  editSubscriptionQuery,
  getRunningSubscriptionProgress,
  getRunningSubscriptions,
  getSubscriptionSites,
  getSubscriptions,
  listCredentialHealth,
  listCredentials,
  listSubscriptionDownloadAttempts,
  listSubscriptionIssues,
  listSubscriptionRuns,
  pauseSubscription,
  pauseSubscriptionQuery,
  pixivOAuthExchange,
  pixivOAuthStart,
  renameSubscription,
  resetSubscription,
  resetSubscriptionQuery,
  retrySubscriptionFailedPost,
  runSubscription,
  runSubscriptionQuery,
  setCredential,
  setSubscriptionAutoCollections,
  stopSubscription,
  stopSubscriptionQuery,
} from '../platform/subscriptionApi';
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
import { groupFailedPostAttempts } from '../shared/lib/subscriptionHelpers';
import type { SubscriptionWorkspaceSnapshot } from '../shared/types/subscriptionsWorkspace';

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
      getSubscriptions(),
      getSubscriptionSites(),
      listCredentials(),
      listCredentialHealth(),
      getRunningSubscriptions(),
      getRunningSubscriptionProgress(),
    ]);

    const metricsEntries = await Promise.all(
      subscriptions.map(async (subscription) => {
        const [issues, attempts] = await Promise.all([
          listSubscriptionIssues(subscription.id, null, 100),
          listSubscriptionDownloadAttempts(subscription.id, null, 100),
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
      getRunningSubscriptions(),
      getRunningSubscriptionProgress(),
    ]);
    return { runningSubscriptionIds, runningProgress };
  },

  async listRuns(subscriptionId: string): Promise<SubscriptionRunRecord[]> {
    return listSubscriptionRuns(subscriptionId, 20);
  },

  async listIssues(subscriptionId: string): Promise<SubscriptionIssueRecord[]> {
    return listSubscriptionIssues(subscriptionId, null, 50);
  },

  async listFailedPosts(subscription: SubscriptionInfo): Promise<FailedPostGroup[]> {
    const attempts = await listSubscriptionDownloadAttempts(subscription.id, null, 100);
    return groupFailedPostAttempts(attempts, subscription.queries);
  },

  getSites(): Promise<SubscriptionSiteInfo[]> {
    return getSubscriptionSites();
  },

  getSubscriptions(): Promise<SubscriptionInfo[]> {
    return getSubscriptions();
  },

  create(input: {
    name: string;
    initial_post_limit?: number | null;
    periodic_post_limit?: number | null;
  }): Promise<SubscriptionInfo> {
    return createSubscription(input);
  },

  createGroup(name: string, schedule?: string | null): Promise<unknown> {
    return createGroup(name, schedule);
  },

  rename(id: string, name: string): Promise<void> {
    return renameSubscription(id, name);
  },

  delete(id: string): Promise<void> {
    return deleteSubscription(id);
  },

  pause(id: string, paused: boolean): Promise<void> {
    return pauseSubscription(id, paused);
  },

  reset(id: string): Promise<void> {
    return resetSubscription(id);
  },

  run(id: string): Promise<void> {
    return runSubscription(id);
  },

  stop(id: string): Promise<void> {
    return stopSubscription(id);
  },

  setAutoCollections(id: string, autoCollections: boolean): Promise<void> {
    return setSubscriptionAutoCollections(id, autoCollections);
  },

  addQuery(
    subscriptionId: string,
    siteId: string,
    queryText: string,
    notes?: string | null,
  ): Promise<SubscriptionInfo['queries'][number]> {
    return addSubscriptionQuery(subscriptionId, siteId, queryText, notes);
  },

  editQuery(
    id: number,
    siteId: string,
    queryText: string,
    displayName?: string | null,
    notes?: string | null,
  ): Promise<void> {
    return editSubscriptionQuery(id, siteId, queryText, displayName, notes);
  },

  deleteQuery(id: string): Promise<void> {
    return deleteSubscriptionQuery(id);
  },

  pauseQuery(id: string, paused: boolean): Promise<void> {
    return pauseSubscriptionQuery(id, paused);
  },

  resetQuery(id: string): Promise<void> {
    return resetSubscriptionQuery(id);
  },

  runQuery(subscriptionId: string, queryId: string): Promise<void> {
    return runSubscriptionQuery(subscriptionId, queryId);
  },

  stopQuery(subscriptionId: string, queryId: string): Promise<void> {
    return stopSubscriptionQuery(subscriptionId, queryId);
  },

  retryFailedPost(input: {
    subscription_id: string;
    query_id: string;
    site_id: string;
    post_id: string;
  }): Promise<void> {
    return retrySubscriptionFailedPost(input);
  },

  listCredentials(): Promise<CredentialDomain[]> {
    return listCredentials();
  },

  listCredentialHealth(): Promise<CredentialHealth[]> {
    return listCredentialHealth();
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
    return setCredential(input);
  },

  deleteCredential(siteCategory: string): Promise<void> {
    return deleteCredential(siteCategory);
  },

  pixivOAuthStart(): Promise<PixivOAuthStartResult> {
    return pixivOAuthStart();
  },

  pixivOAuthExchange(code: string, codeVerifier: string, phpsessid?: string | null): Promise<PixivOAuthExchangeResult> {
    return pixivOAuthExchange(code, codeVerifier, phpsessid);
  },

  openExternalUrl(url: string): Promise<void> {
    return openExternalUrl(url);
  },
};
