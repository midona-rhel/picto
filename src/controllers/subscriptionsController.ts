import { openExternalUrl } from '../platform/shellApi';
import {
  addSubscriptionQuery,
  cleanupGalleryImport,
  createSubscription,
  deleteCredential,
  deleteSubscription,
  deleteSubscriptionQuery,
  editSubscriptionQuery,
  getSubscriptionOverview,
  getSubscriptionProgress,
  getSubscriptionRunActivity,
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
  startGalleryImport,
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
import type { SubscriptionRunActivity } from '../shared/types/generated/application/SubscriptionRunActivity';

function deriveLastActivityAt(subscription: SubscriptionInfo): string | null {
  let latest: string | null = null;
  for (const query of subscription.queries) {
    if (!query.last_check_time) continue;
    if (!latest || query.last_check_time > latest) latest = query.last_check_time;
  }
  return latest;
}

export const subscriptionsController = {
  async loadWorkspaceSnapshot(): Promise<SubscriptionWorkspaceSnapshot & { covers: Map<string, SubscriptionCover> }> {
    const [overview, sites, credentials, credentialHealth] = await Promise.all([
      getSubscriptionOverview(),
      getSubscriptionSites(),
      listCredentials(),
      listCredentialHealth(),
    ]);

    return {
      subscriptions: overview.subscriptions,
      sites,
      credentials,
      credentialHealth,
      runningSubscriptionIds: overview.runningSubscriptionIds,
      runningProgress: overview.runningProgress,
      listMetrics: Object.fromEntries(overview.subscriptions.map((subscription) => [
        subscription.id,
        {
          failedPostCount: 0,
          openIssueCount: overview.openIssueCounts[subscription.id] ?? 0,
          lastActivityAt: deriveLastActivityAt(subscription),
        },
      ])),
      covers: overview.covers,
    };
  },

  async refreshRuntimeState(runningSubscriptions: SubscriptionInfo[]): Promise<{
    runningSubscriptionIds: string[];
    runningProgress: SubscriptionProgressEvent[];
  }> {
    const progress = await Promise.all(runningSubscriptions.map(getSubscriptionProgress));
    const runningProgress = progress.filter((entry): entry is SubscriptionProgressEvent => entry !== null);
    const runningSubscriptionIds = runningProgress.map((entry) => entry.subscription_id);
    return { runningSubscriptionIds, runningProgress };
  },

  async listRuns(subscriptionId: string): Promise<SubscriptionRunRecord[]> {
    return listSubscriptionRuns(subscriptionId, 20);
  },

  getRunActivity(runId: number): Promise<SubscriptionRunActivity> {
    return getSubscriptionRunActivity(runId, 1);
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

  startGalleryImport(url: string): Promise<void> {
    return startGalleryImport(url);
  },

  cleanupGalleryImport(id: string): Promise<void> {
    return cleanupGalleryImport(id);
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
