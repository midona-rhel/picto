import { invoke } from './ipc';
import type {
  AuthSessionState,
  CredentialDomain,
  CredentialHealth,
  PixivOAuthExchangeResult,
  PixivOAuthStartResult,
  SubscriptionDownloadAttemptPage,
  SubscriptionBulkRetryResult,
  SubscriptionInfo,
  SubscriptionIssuePage,
  SubscriptionProgressEvent,
  SubscriptionRunRecord,
  SubscriptionSiteInfo,
} from '../shared/types/subscriptions';

export function getSubscriptionSites(): Promise<SubscriptionSiteInfo[]> {
  return invoke<SubscriptionSiteInfo[]>('get_sites');
}

export interface SubscriptionCoverRecord {
  subscription_id: string;
  entity_hash: string;
}

/** Newest downloaded file per subscription, used as its grid cover. */
export function getSubscriptionCovers(): Promise<SubscriptionCoverRecord[]> {
  return invoke<SubscriptionCoverRecord[]>('get_subscription_covers', {});
}

export interface TagSuggestion {
  name: string;
  post_count: number | null;
  category: string | null;
}

/** Booru tag autocomplete. Empty for unsupported sites or on any failure. */
export function suggestSiteTags(
  siteId: string,
  prefix: string,
  limit?: number,
): Promise<TagSuggestion[]> {
  return invoke<TagSuggestion[]>('suggest_site_tags', {
    site_id: siteId,
    prefix,
    limit: limit ?? null,
  });
}

export function getSubscriptions(): Promise<SubscriptionInfo[]> {
  return invoke<SubscriptionInfo[]>('get_subscriptions');
}

export function setSubscriptionSchedule(id: string, schedule: string): Promise<void> {
  return invoke<void>('set_subscription_schedule', { id, schedule });
}

export function createSubscription(params: {
  name: string;
  initial_post_limit?: number | null;
  periodic_post_limit?: number | null;
}): Promise<SubscriptionInfo> {
  return invoke<SubscriptionInfo>('create_subscription', params as unknown as Record<string, unknown>);
}

export function deleteSubscription(id: string): Promise<void> {
  return invoke<void>('delete_subscription', { id });
}

export function renameSubscription(id: string, name: string): Promise<void> {
  return invoke<void>('rename_subscription', { id, name });
}

export function pauseSubscription(id: string, paused: boolean): Promise<void> {
  return invoke<void>('pause_subscription', { id, paused });
}

export function runSubscription(id: string): Promise<void> {
  return invoke<void>('run_subscription', { id });
}

export function stopSubscription(id: string): Promise<void> {
  return invoke<void>('stop_subscription', { id });
}

export function resetSubscription(id: string): Promise<void> {
  return invoke<void>('reset_subscription', { id });
}

export function resetSubscriptionQuery(id: string): Promise<void> {
  return invoke<void>('reset_subscription_query', { id });
}

export function addSubscriptionQuery(
  subscriptionId: string,
  siteId: string,
  queryText: string,
  notes?: string | null,
): Promise<SubscriptionInfo['queries'][number]> {
  return invoke<SubscriptionInfo['queries'][number]>('add_subscription_query', {
    subscription_id: subscriptionId,
    site_id: siteId,
    query_text: queryText,
    notes: notes ?? null,
  });
}

export function editSubscriptionQuery(
  id: number,
  siteId: string,
  queryText: string,
  displayName?: string | null,
  notes?: string | null,
): Promise<void> {
  return invoke<void>('edit_subscription_query', {
    id,
    site_id: siteId,
    query_text: queryText,
    display_name: displayName ?? null,
    notes: notes ?? null,
  });
}

export function deleteSubscriptionQuery(id: string): Promise<void> {
  return invoke<void>('delete_subscription_query', { id });
}

export function pauseSubscriptionQuery(id: string, paused: boolean): Promise<void> {
  return invoke<void>('pause_subscription_query', { id, paused });
}

export function runSubscriptionQuery(subscriptionId: string, queryId: string): Promise<void> {
  return invoke<void>('run_subscription_query', {
    subscription_id: subscriptionId,
    query_id: queryId,
  });
}

export function stopSubscriptionQuery(subscriptionId: string, queryId: string): Promise<void> {
  return invoke<void>('stop_subscription_query', {
    subscription_id: subscriptionId,
    query_id: queryId,
  });
}

export function retrySubscriptionFailedPost(input: {
  subscription_id: string;
  query_id: string;
  post_id: string;
}): Promise<void> {
  return invoke<void>('retry_subscription_failed_post', input as unknown as Record<string, unknown>);
}

export function retrySubscriptionFailedPosts(subscriptionId: string): Promise<SubscriptionBulkRetryResult> {
  return invoke<SubscriptionBulkRetryResult>('retry_subscription_failed_posts', {
    subscription_id: subscriptionId,
  });
}

export function getRunningSubscriptions(): Promise<string[]> {
  return invoke<string[]>('get_running_subscriptions');
}

export function getRunningSubscriptionProgress(): Promise<SubscriptionProgressEvent[]> {
  return invoke<SubscriptionProgressEvent[]>('get_running_subscription_progress');
}

export function listSubscriptionRuns(subscriptionId: string, limit = 20): Promise<SubscriptionRunRecord[]> {
  return invoke<SubscriptionRunRecord[]>('list_subscription_runs', {
    subscription_id: subscriptionId,
    limit,
  });
}

export function listSubscriptionIssues(
  subscriptionId: string,
  queryId?: string | null,
  limit = 50,
  cursor?: number | null,
): Promise<SubscriptionIssuePage> {
  return invoke<SubscriptionIssuePage>('list_subscription_issues', {
    subscription_id: subscriptionId,
    query_id: queryId ?? null,
    cursor: cursor ?? null,
    limit,
  });
}

export function listSubscriptionDownloadAttempts(
  subscriptionId: string,
  queryId?: string | null,
  limit = 50,
  cursor?: number | null,
): Promise<SubscriptionDownloadAttemptPage> {
  return invoke<SubscriptionDownloadAttemptPage>('list_subscription_download_attempts', {
    subscription_id: subscriptionId,
    query_id: queryId ?? null,
    cursor: cursor ?? null,
    limit,
  });
}

export function listCredentials(): Promise<CredentialDomain[]> {
  return invoke<CredentialDomain[]>('list_credentials');
}

export function listCredentialHealth(): Promise<CredentialHealth[]> {
  return invoke<CredentialHealth[]>('list_credential_health');
}

export function setCredential(input: {
  site_category: string;
  credential_type: string;
  display_name?: string | null;
  username?: string | null;
  password?: string | null;
  cookies?: Record<string, string> | null;
  oauth_token?: string | null;
}): Promise<void> {
  return invoke<void>('set_credential', input as unknown as Record<string, unknown>);
}

export function deleteCredential(siteCategory: string): Promise<void> {
  return invoke<void>('delete_credential', { site_category: siteCategory });
}

export function pixivOAuthStart(): Promise<PixivOAuthStartResult> {
  return invoke<PixivOAuthStartResult>('pixiv_oauth_start');
}

export function startAuthSession(siteCategory: string, startUrl?: string | null): Promise<AuthSessionState> {
  return invoke<AuthSessionState>('auth_session_start', {
    site_category: siteCategory,
    start_url: startUrl ?? null,
  });
}

export function cancelAuthSession(): Promise<void> {
  return invoke<void>('auth_session_cancel');
}

export function pixivOAuthExchange(
  code: string,
  codeVerifier: string,
  phpsessid?: string | null,
): Promise<PixivOAuthExchangeResult> {
  return invoke<PixivOAuthExchangeResult>('pixiv_oauth_exchange', {
    code,
    code_verifier: codeVerifier,
    phpsessid: phpsessid ?? null,
  });
}
