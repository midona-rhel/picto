import { openExternalUrl } from '../platform/shellApi';
import {
  addSubscriptionQuery,
  createGroup,
  createSubscription,
  deleteCredential,
  deleteGroup,
  deleteSubscription,
  deleteSubscriptionQuery,
  editSubscriptionQuery,
  getGroups,
  getRunningSubscriptionProgress,
  getRunningSubscriptions,
  getSubscriptionCovers,
  getSubscriptionSites,
  getSubscriptions,
  listCredentialHealth,
  listCredentials,
  listSubscriptionCollections,
  listSubscriptionDownloadAttempts,
  listSubscriptionIssues,
  listSubscriptionRuns,
  pauseSubscription,
  pauseSubscriptionQuery,
  pixivOAuthExchange,
  pixivOAuthStart,
  renameGroup,
  renameSubscription,
  resetSubscription,
  resetSubscriptionQuery,
  retrySubscriptionFailedPost,
  runGroup,
  runSubscription,
  runSubscriptionQuery,
  setCredential,
  setSubscriptionSchedule,
  setSubscriptionAutoCollections,
  setSubscriptionGroup,
  stopGroup,
  stopSubscription,
  stopSubscriptionQuery,
  suggestSiteTags,
  verifySubscriptionSite,
} from '../platform/subscriptionApi';
import type {
  SiteVerificationReport,
  SubscriptionCollectionRecord,
  TagSuggestion,
} from '../platform/subscriptionApi';
import type {
  CredentialDomain,
  CredentialHealth,
  CredentialType,
  PixivOAuthExchangeResult,
  PixivOAuthStartResult,
  FailedPostGroup,
  SubscriptionGroupInfo,
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
    const [subscriptions, groups, sites, credentials, credentialHealth, runningSubscriptionIds, runningProgress] = await Promise.all([
      getSubscriptions(),
      getGroups(),
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
      groups,
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
    group_id?: number | null;
    initial_post_limit?: number | null;
    periodic_post_limit?: number | null;
  }): Promise<SubscriptionInfo> {
    return createSubscription(input);
  },

  getGroups(): Promise<SubscriptionGroupInfo[]> {
    return getGroups();
  },

  createGroup(name: string): Promise<SubscriptionGroupInfo> {
    return createGroup(name);
  },

  renameGroup(id: string, name: string): Promise<void> {
    return renameGroup(id, name);
  },

  deleteGroup(id: string): Promise<void> {
    return deleteGroup(id);
  },

  setSchedule(id: string, schedule: string): Promise<void> {
    return setSubscriptionSchedule(id, schedule);
  },

  runGroup(id: string): Promise<void> {
    return runGroup(id);
  },

  stopGroup(id: string): Promise<void> {
    return stopGroup(id);
  },

  setSubscriptionGroup(subscriptionId: string, groupId: number | null): Promise<void> {
    return setSubscriptionGroup(subscriptionId, groupId);
  },

  suggestSiteTags(siteId: string, prefix: string, limit?: number): Promise<TagSuggestion[]> {
    return suggestSiteTags(siteId, prefix, limit);
  },

  verifySite(siteId: string, query?: string | null): Promise<SiteVerificationReport> {
    return verifySubscriptionSite(siteId, query, 2);
  },

  listCollections(subscriptionId: string): Promise<SubscriptionCollectionRecord[]> {
    return listSubscriptionCollections(subscriptionId);
  },

  /** Newest downloaded file hash per subscription id, for grid covers. */
  async getCovers(): Promise<Map<string, string>> {
    const records = await getSubscriptionCovers();
    return new Map(records.map((record) => [record.subscription_id, record.entity_hash]));
  },

  /** Retry failed posts one at a time — the backend serializes per-site anyway. */
  async retryFailedPosts(
    posts: Array<{ subscription_id: string; query_id: string; site_id: string; post_id: string }>,
  ): Promise<{ ok: number; failed: number }> {
    let ok = 0;
    let failed = 0;
    for (const post of posts) {
      try {
        await retrySubscriptionFailedPost(post);
        ok++;
      } catch {
        failed++;
      }
    }
    return { ok, failed };
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
