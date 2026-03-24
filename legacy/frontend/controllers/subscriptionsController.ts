import { api } from '#desktop/api';
import { canStartTaskFamily, isTaskFamilyRunning } from '../state-legacy/taskStore';
import type {
  CredentialDomain,
  CredentialHealth,
  CredentialType,
  SiteMetadataSchema,
  SiteMetadataValidationResult,
  SubscriptionGroupInfo as BackendGroupInfo,
  SubscriptionInfo as BackendSubInfo,
  SubscriptionQueryInfo as BackendQueryInfo,
  SubscriptionSiteInfo,
} from '../shared/types/api';
import type { SubscriptionProgressEvent } from '../shared/types/api/core';
import type {
  SubscriptionGroupInfo,
  SubInfo,
  SubscriptionQueryInfo,
} from '../features/subscriptions/types';

// ── DTO normalization ──────────────────────────────────────────
// Converts backend-shaped DTOs to the frontend's enriched types in one place.

function normalizeQuery(query: BackendQueryInfo): SubscriptionQueryInfo {
  const q = query as BackendQueryInfo & { posts_found?: number; last_seen_id?: string | null };
  return {
    id: q.id,
    query_text: q.query_text,
    display_name: q.display_name,
    paused: q.paused,
    last_check_time: q.last_check_time,
    files_found: q.files_found,
    posts_found: q.posts_found ?? q.files_found,
    completed_initial_run: q.completed_initial_run,
    last_seen_id: q.last_seen_id ?? null,
    resume_cursor: q.resume_cursor ?? null,
    resume_strategy: q.resume_strategy ?? null,
  };
}

function normalizeSub(sub: BackendSubInfo): SubInfo {
  const s = sub as BackendSubInfo & { site_plugin_id?: string; auto_collections?: boolean };
  return {
    id: s.id,
    name: s.name,
    site_id: s.site_id,
    site_plugin_id: s.site_plugin_id ?? s.site_id,
    paused: s.paused,
    group_id: s.group_id,
    initial_post_limit: s.initial_post_limit,
    periodic_post_limit: s.periodic_post_limit,
    auto_collections: s.auto_collections ?? true,
    created_at: s.created_at,
    total_files: s.total_files,
    queries: s.queries.map(normalizeQuery),
  };
}

function normalizeGroup(group: BackendGroupInfo): SubscriptionGroupInfo {
  return {
    id: group.id,
    name: group.name,
    schedule: group.schedule,
    created_at: group.created_at,
    total_files: group.total_files,
    subscriptions: group.subscriptions.map(normalizeSub),
  };
}

export const subscriptionsController = {
  // ── Groups ────────────────────────────────────────────────────

  async listGroups(): Promise<SubscriptionGroupInfo[]> {
    const raw = await api.groups.list() as BackendGroupInfo[];
    return raw.map(normalizeGroup);
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

  async runGroup(id: string) {
    const check = canStartTaskFamily('subscription_run');
    if (!check.allowed) throw new Error(check.reason ?? 'Subscription run blocked');
    await api.groups.run(id);
  },

  async stopGroup(id: string) {
    await api.groups.stop(id);
  },

  setGroupSchedule(id: string, schedule: string) {
    return api.groups.setSchedule(id, schedule);
  },

  // ── Task state ────────────────────────────────────────────────

  canStart(): { allowed: boolean; reason?: string } {
    return canStartTaskFamily('subscription_run');
  },

  isRunning(): boolean {
    return isTaskFamilyRunning('subscription_run');
  },

  // Subscription run progress is tracked entirely via backend RuntimeTask events.
  // No local familyProgress tracking needed.

  getRunning(): Promise<string[]> {
    return api.subscriptions.getRunning();
  },

  getRunningProgress(): Promise<SubscriptionProgressEvent[]> {
    return api.subscriptions.getRunningProgress();
  },

  // ── Subscriptions ─────────────────────────────────────────────

  create(params: {
    name: string;
    site_id: string;
    queries: string[];
    group_id?: number;
    initial_post_limit?: number;
    periodic_post_limit?: number;
  }) {
    return api.subscriptions.create(params);
  },

  delete(id: string, deleteFiles?: boolean) {
    return api.subscriptions.delete(id, deleteFiles);
  },

  rename(id: string, name: string) {
    return api.subscriptions.rename(id, name);
  },

  pause(id: string, paused: boolean) {
    return api.subscriptions.pause(id, paused);
  },

  async run(id: string) {
    const check = canStartTaskFamily('subscription_run');
    if (!check.allowed) throw new Error(check.reason ?? 'Subscription run blocked');
    await api.subscriptions.run(id);
  },

  async stop(id: string) {
    await api.subscriptions.stop(id);
  },

  reset(id: string) {
    return api.subscriptions.reset(id);
  },

  setAutoCollections(subscriptionId: string, autoCollections: boolean) {
    return api.subscriptions.setAutoCollections(subscriptionId, autoCollections);
  },

  // ── Queries ───────────────────────────────────────────────────

  async addQuery(subscriptionId: string, queryText: string): Promise<SubscriptionQueryInfo> {
    const raw = await api.subscriptions.addQuery(subscriptionId, queryText);
    return normalizeQuery(raw);
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

  runQuery(subscriptionId: string, queryId: string) {
    const check = canStartTaskFamily('subscription_run');
    if (!check.allowed) return Promise.reject(new Error(check.reason ?? 'Subscription run blocked'));
    return api.subscriptions.runQuery(subscriptionId, queryId);
  },

  // ── Sites & Credentials ──────────────────────────────────────

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

  // ── Pixiv OAuth ──────────────────────────────────────────────

  pixivOAuthStart(): Promise<{ login_url: string; code_verifier: string }> {
    return api.subscriptions.pixivOAuthStart();
  },

  pixivOAuthPopup(loginUrl: string): Promise<{ code: string; phpsessid: string | null }> {
    return api.subscriptions.pixivOAuthPopup(loginUrl);
  },

  pixivOAuthExchange(code: string, codeVerifier: string, phpsessid?: string | null): Promise<{ ok: boolean }> {
    return api.subscriptions.pixivOAuthExchange(code, codeVerifier, phpsessid);
  },
};
