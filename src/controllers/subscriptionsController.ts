import { api } from '#desktop/api';
import { canStartTaskFamily, isTaskFamilyRunning, useTaskStore, type TaskProgress } from '../state/taskStore';
import type {
  CredentialDomain,
  CredentialHealth,
  CredentialType,
  SiteMetadataSchema,
  SiteMetadataValidationResult,
  SubscriptionGroupInfo,
  SubscriptionInfo,
  SubscriptionQueryInfo,
  SubscriptionSiteInfo,
} from '../shared/types/api';
import type { SubscriptionProgressEvent } from '../shared/types/api/core';

export const subscriptionsController = {
  listGroups(): Promise<SubscriptionGroupInfo[]> {
    return api.groups.list() as Promise<SubscriptionGroupInfo[]>;
  },

  createGroup(name: string, schedule?: string) {
    return api.groups.create(name, schedule);
  },

  renameGroup(id: string, name: string) {
    return api.groups.rename(id, name);
  },

  deleteGroup(id: string, deleteFiles?: boolean) {
    return api.groups.delete(id, deleteFiles);
  },

  /** Check if a subscription run can start. */
  canStart(): { allowed: boolean; reason?: string } {
    return canStartTaskFamily('subscription_run');
  },

  /** Whether any subscription/group is currently running. */
  isRunning(): boolean {
    return isTaskFamilyRunning('subscription_run');
  },

  /** Called by event listeners / stateChangeStore when subscription progress updates. */
  updateProgress(progress: TaskProgress) {
    const store = useTaskStore.getState();
    if (!store.familyProgress.subscription_run.running) {
      store.startFamily('subscription_run');
    }
    store.updateFamilyProgress('subscription_run', progress);
  },

  finishRun() {
    useTaskStore.getState().finishFamily('subscription_run');
  },

  runGroup(id: string) {
    const check = canStartTaskFamily('subscription_run');
    if (!check.allowed) return Promise.reject(new Error(check.reason ?? 'Subscription run blocked'));
    useTaskStore.getState().startFamily('subscription_run');
    return api.groups.run(id);
  },

  stopGroup(id: string) {
    return api.groups.stop(id);
  },

  setGroupSchedule(id: string, schedule: string) {
    return api.groups.setSchedule(id, schedule);
  },

  listSubscriptions(): Promise<SubscriptionInfo[]> {
    return api.subscriptions.list();
  },

  getRunning(): Promise<string[]> {
    return api.subscriptions.getRunning();
  },

  getRunningProgress(): Promise<SubscriptionProgressEvent[]> {
    return api.subscriptions.getRunningProgress();
  },

  getSites(): Promise<SubscriptionSiteInfo[]> {
    return api.subscriptions.getSites();
  },

  getSiteMetadataSchema(siteId: string): Promise<SiteMetadataSchema> {
    return api.subscriptions.getSiteMetadataSchema(siteId);
  },

  validateSiteMetadata(params: {
    site_id: string;
    sample_url?: string;
    sample_metadata_json?: Record<string, unknown> | null;
  }): Promise<SiteMetadataValidationResult> {
    return api.subscriptions.validateSiteMetadata(params);
  },

  listCredentials(): Promise<CredentialDomain[]> {
    return api.subscriptions.listCredentials();
  },

  listCredentialHealth(): Promise<CredentialHealth[]> {
    return api.subscriptions.listCredentialHealth();
  },

  setCredential(params: {
    site_category: string;
    credential_type: CredentialType;
    display_name?: string | null;
    username?: string | null;
    password?: string | null;
    cookies?: Record<string, string> | null;
    oauth_token?: string | null;
  }) {
    return api.subscriptions.setCredential(params);
  },

  deleteCredential(siteCategory: string) {
    return api.subscriptions.deleteCredential(siteCategory);
  },

  pixivOAuthStart(): Promise<{ login_url: string; code_verifier: string }> {
    return api.subscriptions.pixivOAuthStart();
  },

  pixivOAuthPopup(loginUrl: string): Promise<{ code: string; phpsessid: string | null }> {
    return api.subscriptions.pixivOAuthPopup(loginUrl);
  },

  pixivOAuthExchange(code: string, codeVerifier: string, phpsessid?: string | null): Promise<{ ok: boolean }> {
    return api.subscriptions.pixivOAuthExchange(code, codeVerifier, phpsessid);
  },

  createSubscription(params: {
    name: string;
    site_id: string;
    queries: string[];
    group_id?: number;
    initial_post_limit?: number;
    periodic_post_limit?: number;
  }) {
    return api.subscriptions.create(params);
  },

  deleteSubscription(id: string, deleteFiles?: boolean) {
    return api.subscriptions.delete(id, deleteFiles);
  },

  renameSubscription(id: string, name: string) {
    return api.subscriptions.rename(id, name);
  },

  pauseSubscription(id: string, paused: boolean) {
    return api.subscriptions.pause(id, paused);
  },

  runSubscription(id: string) {
    const check = canStartTaskFamily('subscription_run');
    if (!check.allowed) return Promise.reject(new Error(check.reason ?? 'Subscription run blocked'));
    useTaskStore.getState().startFamily('subscription_run');
    return api.subscriptions.run(id);
  },

  stopSubscription(id: string) {
    return api.subscriptions.stop(id);
  },

  resetSubscription(id: string) {
    return api.subscriptions.reset(id);
  },

  addQuery(subscriptionId: string, queryText: string): Promise<SubscriptionQueryInfo> {
    return api.subscriptions.addQuery(subscriptionId, queryText);
  },

  deleteQuery(id: string) {
    return api.subscriptions.deleteQuery(id);
  },

  editQuery(id: number, queryText: string, displayName?: string | null) {
    return api.subscriptions.editQuery(id, queryText, displayName);
  },

  pauseQuery(id: string, paused: boolean) {
    return api.subscriptions.pauseQuery(id, paused);
  },

  setAutoCollections(subscriptionId: string, autoCollections: boolean) {
    return api.subscriptions.setAutoCollections(subscriptionId, autoCollections);
  },

  runQuery(subscriptionId: string, queryId: string) {
    const check = canStartTaskFamily('subscription_run');
    if (!check.allowed) return Promise.reject(new Error(check.reason ?? 'Subscription run blocked'));
    useTaskStore.getState().startFamily('subscription_run');
    return api.subscriptions.runQuery(subscriptionId, queryId);
  },
};
