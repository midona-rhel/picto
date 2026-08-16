import { openExternalUrl } from '../platform/shellApi';
import {
  addSubscriptionQuery,
  createSubscription,
  deleteCredential,
  deleteSubscription,
  deleteSubscriptionQuery,
  editSubscriptionQuery,
  getRunningSubscriptionProgress,
  getRunningSubscriptions,
  getSubscriptionCovers,
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
  retrySubscriptionFailedPosts,
  runSubscription,
  runSubscriptionQuery,
  setCredential,
  setSubscriptionSchedule,
  stopSubscription,
  stopSubscriptionQuery,
  suggestSiteTags,
} from '../platform/subscriptionApi';
import type { TagSuggestion } from '../platform/subscriptionApi';
import type {
  CredentialDomain,
  CredentialHealth,
  CredentialType,
  PixivOAuthExchangeResult,
  PixivOAuthStartResult,
  SubscriptionInfo,
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
          listSubscriptionIssues(subscription.id, null, 1),
          listSubscriptionDownloadAttempts(subscription.id, null, 1),
        ]);
        return [
          subscription.id,
          {
            failedPostCount: attempts.failed_post_count,
            openIssueCount: issues.total_count,
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

  async listIssues(subscriptionId: string, cursor?: number | null) {
    return listSubscriptionIssues(subscriptionId, null, 50, cursor);
  },

  async listFailedPosts(subscription: SubscriptionInfo, cursor?: number | null) {
    const page = await listSubscriptionDownloadAttempts(subscription.id, null, 100, cursor);
    return {
      attempts: page.items,
      failedPosts: groupFailedPostAttempts(page.items, subscription.queries),
      nextCursor: page.next_cursor,
      totalCount: page.failed_post_count,
      retryableCount: page.retryable_post_count,
    };
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

  setSchedule(id: string, schedule: string): Promise<void> {
    return setSubscriptionSchedule(id, schedule);
  },

  suggestSiteTags(siteId: string, prefix: string, limit?: number): Promise<TagSuggestion[]> {
    return suggestSiteTags(siteId, prefix, limit);
  },

  /** Newest downloaded file hash per subscription id, for grid covers. */
  async getCovers(): Promise<Map<string, string>> {
    const records = await getSubscriptionCovers();
    return new Map(records.map((record) => [record.subscription_id, record.entity_hash]));
  },

  async retryFailedPosts(subscriptionId: string) {
    const result = await retrySubscriptionFailedPosts(subscriptionId);
    if (result.failed > 0) {
      throw new Error(
        `Queued ${result.queued} failed post${result.queued === 1 ? '' : 's'}, but ${result.failed} could not be queued.`,
      );
    }
    return result;
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
