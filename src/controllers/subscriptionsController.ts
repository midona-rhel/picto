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
  getSubscriptionCoverCandidates,
  getSubscriptionSites,
  getSubscriptions,
  listCredentialHealth,
  listCredentials,
  listSubscriptionIssues,
  listSubscriptionRuns,
  pauseSubscription,
  pauseSubscriptionQuery,
  setSubscriptionQueryGrouping,
  renameSubscription,
  resetSubscription,
  runSubscription,
  setSubscriptionSchedule,
  setSubscriptionPostsPerRun,
  setSubscriptionDestination,
  setSubscriptionCover,
  stopSubscription,
} from '../platform/subscriptionApi';
import type { IssueCursor } from '../shared/types/generated/application/IssueCursor';
import type {
  CredentialDomain,
  CredentialHealth,
  SubscriptionInfo,
  SubscriptionCover,
  SubscriptionProgressEvent,
  SubscriptionRunRecord,
  SubscriptionSiteInfo,
} from '../shared/types/subscriptions';
import type { SubscriptionCoverCandidateCursor } from '../shared/types/generated/application/SubscriptionCoverCandidateCursor';
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
        const issues = await listSubscriptionIssues(subscription.id, null, 1);
        return [
          subscription.id,
          {
            failedPostCount: 0,
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

  async listIssues(subscriptionId: string, cursor?: IssueCursor | null) {
    return listSubscriptionIssues(subscriptionId, null, 50, cursor);
  },

  getSites(): Promise<SubscriptionSiteInfo[]> {
    return getSubscriptionSites();
  },

  getSubscriptions(): Promise<SubscriptionInfo[]> {
    return getSubscriptions();
  },

  create(input: {
    name: string;
  }): Promise<SubscriptionInfo> {
    return createSubscription(input);
  },

  setSchedule(id: string, schedule: string): Promise<void> {
    return setSubscriptionSchedule(id, schedule);
  },

  setPostsPerRun(id: string, postsPerRun: number): Promise<void> {
    return setSubscriptionPostsPerRun(id, postsPerRun);
  },

  setDestination(
    id: string,
    destination: { target_folder_ids: number[]; automatic_tags: string[] },
  ): Promise<void> {
    return setSubscriptionDestination(id, destination);
  },

  /** Newest downloaded file hash per subscription id, for grid covers. */
  async getCovers(): Promise<Map<string, SubscriptionCover>> {
    return getSubscriptionCovers();
  },

  getCoverCandidates(
    id: string,
    cursor: SubscriptionCoverCandidateCursor | null = null,
    limit = 200,
  ) {
    return getSubscriptionCoverCandidates(id, cursor, limit);
  },

  setCover(id: string, cover: { media_item_id: number; focus_x: number; focus_y: number; zoom_percent: number }): Promise<void> {
    return setSubscriptionCover(id, cover);
  },

  rename(id: string, name: string): Promise<void> {
    return renameSubscription(id, name);
  },

  delete(id: string): Promise<void> {
    return deleteSubscription(id);
  },

  reset(id: string): Promise<void> {
    return resetSubscription(id);
  },

  pause(id: string, paused: boolean): Promise<void> {
    return pauseSubscription(id, paused);
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

  setQueryGrouping(id: string, groupPosts: boolean): Promise<void> {
    return setSubscriptionQueryGrouping(id, groupPosts);
  },

  listCredentials(): Promise<CredentialDomain[]> {
    return listCredentials();
  },

  listCredentialHealth(): Promise<CredentialHealth[]> {
    return listCredentialHealth();
  },

  deleteCredential(siteCategory: string): Promise<void> {
    return deleteCredential(siteCategory);
  },

  openExternalUrl(url: string): Promise<void> {
    return openExternalUrl(url);
  },
};
