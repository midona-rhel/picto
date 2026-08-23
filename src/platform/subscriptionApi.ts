import { invoke } from './ipc';
import type { SourceCatalogEntry } from '../shared/types/generated/application/SourceCatalogEntry';
import type { SubscriptionList } from '../shared/types/generated/application/SubscriptionList';
import type { SubscriptionView } from '../shared/types/generated/application/SubscriptionView';
import type { SubscriptionRunList } from '../shared/types/generated/application/SubscriptionRunList';
import type { SubscriptionRunSummary } from '../shared/types/generated/application/SubscriptionRunSummary';
import type { IssuePage } from '../shared/types/generated/application/IssuePage';
import type { IssueCursor } from '../shared/types/generated/application/IssueCursor';
import type { SubscriptionIssue } from '../shared/types/generated/application/SubscriptionIssue';
import type { SubscriptionRunActivity } from '../shared/types/generated/application/SubscriptionRunActivity';
import type { CurrentSubscriptionProgress } from '../shared/types/generated/application/CurrentSubscriptionProgress';
import type { CredentialRecord } from '../shared/types/generated/application/CredentialRecord';
import type { CredentialHealthRecord } from '../shared/types/generated/application/CredentialHealthRecord';
import type { NewSubscriptionQuery } from '../shared/types/generated/application/NewSubscriptionQuery';
import type { NewSubscription } from '../shared/types/generated/application/NewSubscription';
import type { CreatedSubscription } from '../shared/types/generated/application/CreatedSubscription';
import type { CreatedSubscriptionQuery } from '../shared/types/generated/application/CreatedSubscriptionQuery';
import type { CreatedSubscriptionRun } from '../shared/types/generated/application/CreatedSubscriptionRun';
import type { MutationReceipt } from '../shared/types/generated/application/MutationReceipt';
import type {
  AuthSessionState,
  CredentialDomain,
  CredentialHealth,
  SubscriptionInfo,
  SubscriptionIssuePage,
  SubscriptionProgressEvent,
  SubscriptionRunRecord,
  SubscriptionSiteInfo,
} from '../shared/types/subscriptions';

function latest(...values: Array<string | null | undefined>): string | null {
  const present = values.filter((value): value is string => Boolean(value)).sort();
  return present[present.length - 1] ?? null;
}

function mapSite(site: SourceCatalogEntry): SubscriptionSiteInfo {
  return {
    id: site.id,
    name: site.name,
    domain: site.domain,
    credential_owner_site_id: site.credential_owner_site_id,
    example_query: site.example_query,
    supports_query: site.supports_query,
    supports_account: site.supports_account,
    auth_required_for_full_access: site.auth_required_for_full_access,
    auth_strictly_required: site.auth_strictly_required,
    credential_types: site.credential_types as SubscriptionSiteInfo['credential_types'],
  };
}

function mapQuery(query: SubscriptionView['queries'][number]) {
  return {
    id: String(query.query_id),
    site_id: query.site_id,
    query_kind: query.query_kind,
    query_text: query.query_text,
    display_name: query.display_name,
    notes: query.notes,
    paused: query.paused,
    last_check_time: latest(query.last_success_at, query.last_failure_at),
    files_found: query.media_count,
    posts_found: query.post_count,
    completed_initial_run: query.initial_run_complete,
    resume_cursor: null,
    resume_strategy: null,
    last_success_at: query.last_success_at,
    last_failure_at: query.last_failure_at,
    last_failure_kind: query.last_failure_kind,
    last_failure_message: query.last_failure_message,
  };
}

function mapSubscription(subscription: SubscriptionView): SubscriptionInfo {
  return {
    id: String(subscription.subscription_id),
    name: subscription.name,
    schedule: subscription.schedule,
    paused: subscription.paused,
    created_at: '',
    total_files: subscription.media_count,
    queries: subscription.queries.map(mapQuery),
  };
}

function mapProgress(
  subscription: SubscriptionView,
  current: CurrentSubscriptionProgress | null = null,
): SubscriptionProgressEvent | null {
  const runId = current?.run_id ?? subscription.active_run_id;
  const status = current?.status ?? subscription.status;
  if (!runId || !status || !['pending', 'running'].includes(status)) return null;
  const counts = current?.counts ?? subscription.progress;
  const discovered = current?.counts.fetched ?? subscription.progress.discovered;
  return {
    subscription_id: String(subscription.subscription_id),
    subscription_name: subscription.name,
    mode: 'replacement',
    query_id: null,
    query_name: null,
    files_downloaded: counts.downloaded,
    files_skipped: 0,
    queued_for_ingest: current?.counts.queued ?? Math.max(0, discovered - counts.ingested),
    ingesting: 0,
    ingested: counts.ingested,
    reused: 0,
    failed_ingest: counts.failed,
    pages_fetched: 0,
    metadata_validated: 0,
    metadata_invalid: 0,
    last_metadata_error: null,
    status_text: status,
    phase: status,
    current_post_id: null,
    current_post_items: 0,
    posts_processed: 0,
    resume_cursor: null,
    last_error: null,
    finished_status: null,
    failure_kind: null,
    error: null,
  };
}

function mapRun(run: SubscriptionRunSummary): SubscriptionRunRecord {
  return {
    run_id: run.run_id,
    subscription_id: run.subscription_id,
    started_at: run.started_at ?? run.created_at,
    finished_at: run.finished_at,
    status: run.status,
    failure_kind: run.failure_kind,
    error_message: run.error_message,
    files_downloaded: run.counts.downloaded,
    files_skipped: 0,
    metadata_validated: 0,
    metadata_invalid: 0,
  };
}

function recoveryAction(issue: SubscriptionIssue): SubscriptionIssuePage['items'][number]['recovery_action'] {
  if (issue.issue_kind.includes('auth') || issue.issue_kind.includes('credential') || issue.issue_kind === 'unauthorized') {
    return 'fix_credentials';
  }
  if (issue.status === 'open') return 'retry_automatically';
  return 'none';
}

function mapIssue(issue: SubscriptionIssue): SubscriptionIssuePage['items'][number] {
  return {
    issue_id: issue.issue_id,
    issue_key: issue.issue_key,
    subscription_id: issue.subscription_id,
    query_id: issue.query_id,
    issue_kind: issue.issue_kind,
    status: issue.status,
    message: issue.message,
    detail: issue.detail,
    first_seen_at: issue.first_seen_at,
    last_seen_at: issue.last_seen_at,
    resolved_at: issue.resolved_at,
    recovery_action: recoveryAction(issue),
    next_retry_at: null,
  };
}

async function listReplacementSubscriptions(): Promise<SubscriptionList> {
  return invoke<SubscriptionList>('subscriptions.list', {});
}

export function getSubscriptionSites(): Promise<SubscriptionSiteInfo[]> {
  return invoke<SourceCatalogEntry[]>('sources.list', {}).then((sites) => sites.map(mapSite));
}

export function getSubscriptionCovers(): Promise<Map<string, string>> {
  return listReplacementSubscriptions().then((list) => new Map(
    list.subscriptions.flatMap((subscription) => subscription.cover_file_hash
      ? [[String(subscription.subscription_id), subscription.cover_file_hash] as const]
      : []),
  ));
}

export function getSubscriptions(): Promise<SubscriptionInfo[]> {
  return listReplacementSubscriptions().then((list) => list.subscriptions.map(mapSubscription));
}

export function setSubscriptionSchedule(id: string, schedule: string): Promise<void> {
  return invoke<MutationReceipt>('subscriptions.schedule', {
    subscription_id: Number(id),
    schedule,
  }).then(() => undefined);
}

export async function createSubscription(params: {
  name: string;
  site_id: string;
  query_text: string;
}): Promise<SubscriptionInfo> {
  const input: NewSubscription = {
    name: params.name,
    schedule: 'manual',
    initial_post_limit: null,
    periodic_post_limit: null,
    queries: [{
      site_id: params.site_id,
      query_kind: 'tag',
      query_text: params.query_text,
      display_name: null,
      notes: null,
    }],
  };
  const created = await invoke<CreatedSubscription>('subscriptions.create', input);
  const list = await listReplacementSubscriptions();
  const subscription = list.subscriptions.find((entry) => entry.subscription_id === created.subscription_id);
  if (!subscription) throw new Error('Subscription was created but could not be read back.');
  return mapSubscription(subscription);
}

export function deleteSubscription(id: string): Promise<void> {
  return invoke<MutationReceipt>('subscriptions.delete', { subscription_id: Number(id) }).then(() => undefined);
}

export function resetSubscription(id: string): Promise<void> {
  return invoke<MutationReceipt>('subscriptions.reset', { subscription_id: Number(id) }).then(() => undefined);
}

export function renameSubscription(id: string, name: string): Promise<void> {
  return invoke<MutationReceipt>('subscriptions.rename', { subscription_id: Number(id), name }).then(() => undefined);
}

export function pauseSubscription(id: string, paused: boolean): Promise<void> {
  return invoke<MutationReceipt>('subscriptions.pause', { subscription_id: Number(id), paused }).then(() => undefined);
}

export function runSubscription(id: string): Promise<void> {
  return invoke<CreatedSubscriptionRun>('subscriptions.run', { subscription_id: Number(id) }).then(() => undefined);
}

export function stopSubscription(id: string): Promise<void> {
  return invoke<MutationReceipt>('subscriptions.cancel', { subscription_id: Number(id) }).then(() => undefined);
}

export function addSubscriptionQuery(
  subscriptionId: string,
  siteId: string,
  queryText: string,
  notes?: string | null,
): Promise<SubscriptionInfo['queries'][number]> {
  const query: NewSubscriptionQuery = {
    site_id: siteId,
    query_kind: 'tag',
    query_text: queryText,
    display_name: null,
    notes: notes ?? null,
  };
  return invoke<CreatedSubscriptionQuery>('subscriptions.queries.add', {
    subscription_id: Number(subscriptionId),
    query,
  }).then(async (created) => {
    const list = await listReplacementSubscriptions();
    const subscription = list.subscriptions.find((entry) => entry.subscription_id === Number(subscriptionId));
    const result = subscription?.queries.find((entry) => entry.query_id === created.query_id);
    if (!result) throw new Error('Query was added but could not be read back.');
    return mapQuery(result);
  });
}

export function editSubscriptionQuery(
  id: number,
  siteId: string,
  queryText: string,
  displayName?: string | null,
  notes?: string | null,
): Promise<void> {
  const query: NewSubscriptionQuery = {
    site_id: siteId,
    query_kind: 'tag',
    query_text: queryText,
    display_name: displayName ?? null,
    notes: notes ?? null,
  };
  return invoke<MutationReceipt>('subscriptions.queries.update', { query_id: id, query }).then(() => undefined);
}

export function deleteSubscriptionQuery(id: string): Promise<void> {
  return invoke<MutationReceipt>('subscriptions.queries.delete', { query_id: Number(id) }).then(() => undefined);
}

export function pauseSubscriptionQuery(id: string, paused: boolean): Promise<void> {
  return invoke<MutationReceipt>('subscriptions.queries.pause', { query_id: Number(id), paused }).then(() => undefined);
}

export function getRunningSubscriptions(): Promise<string[]> {
  return listReplacementSubscriptions().then((list) => list.subscriptions
    .filter((subscription) => subscription.active_run_id != null && ['pending', 'running'].includes(subscription.status ?? ''))
    .map((subscription) => String(subscription.subscription_id)));
}

export function getRunningSubscriptionProgress(): Promise<SubscriptionProgressEvent[]> {
  return listReplacementSubscriptions().then(async (list) => {
    const active = list.subscriptions.filter((subscription) =>
      subscription.active_run_id != null && ['pending', 'running'].includes(subscription.status ?? ''),
    );
    const progress = await Promise.all(active.map(async (subscription) => {
      const current = await invoke<CurrentSubscriptionProgress | null>('subscriptions.progress.get', {
        subscription_id: subscription.subscription_id,
      });
      return mapProgress(subscription, current);
    }));
    return progress.filter((entry): entry is SubscriptionProgressEvent => entry !== null);
  });
}

export function listSubscriptionRuns(subscriptionId: string, limit = 20): Promise<SubscriptionRunRecord[]> {
  return invoke<SubscriptionRunList>('subscriptions.runs.list', {
    subscription_id: Number(subscriptionId),
    limit,
  }).then((page) => page.runs.map(mapRun));
}

export function listSubscriptionIssues(
  subscriptionId: string,
  _queryId?: string | null,
  limit = 50,
  cursor?: IssueCursor | null,
): Promise<SubscriptionIssuePage> {
  return invoke<IssuePage>('subscriptions.issues.list', {
    subscription_id: Number(subscriptionId),
    query_id: null,
    open_only: false,
    cursor: cursor ?? null,
    limit,
  }).then((page) => ({
    items: page.issues.map(mapIssue),
    next_cursor: page.next_cursor,
    total_count: page.total_count,
  }));
}

export function getSubscriptionRunActivity(runId: number, sourceItemLimit = 100): Promise<SubscriptionRunActivity> {
  return invoke<SubscriptionRunActivity>('subscriptions.runs.get', {
    run_id: runId,
    source_item_limit: sourceItemLimit,
  });
}

export function listCredentials(): Promise<CredentialDomain[]> {
  return invoke<CredentialRecord[]>('auth.credentials.list', {}).then((records) => records.map((record) => ({
    site_category: record.site_id,
    credential_type: record.credential_type as CredentialDomain['credential_type'],
    display_name: record.display_name,
    created_at: record.created_at,
  })));
}

export function listCredentialHealth(): Promise<CredentialHealth[]> {
  return invoke<CredentialHealthRecord[]>('auth.health.list', {}).then((records) => records.map((record) => ({
    site_category: record.site_id,
    health_status: record.status,
    last_checked_at: record.checked_at ?? '',
    last_error: record.last_error,
  })));
}

export function deleteCredential(siteCategory: string): Promise<void> {
  return invoke<MutationReceipt>('auth.credentials.delete', { site_id: siteCategory }).then(() => undefined);
}

export function startAuthSession(siteCategory: string): Promise<AuthSessionState> {
  return invoke<AuthSessionState>('auth_session_start', {
    site_category: siteCategory,
  });
}

export function cancelAuthSession(): Promise<void> {
  return invoke<void>('auth_session_cancel');
}

export function getAuthSessionState(): Promise<AuthSessionState> {
  return invoke<AuthSessionState>('auth_session_state');
}
