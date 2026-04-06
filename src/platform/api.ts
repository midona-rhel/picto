/**
 * Frontend API layer — the only place that knows backend command names.
 * Controllers and features call these methods, never raw invoke().
 */

import { invoke } from './ipc';
import type {
  SidebarTreeResponse, EntityViewQuery, EntityViewPage,
  CanonicalEntityGridItem, CanonicalEntityDetails,
  EntityTarget, MediaEntityPatch, CanonicalTagRecord,
  CanonicalTagRelation, CanonicalNamespaceSummary, SelectionSummary,
  SmartFolderCommandPayload,
} from '../shared/types/canonical';
import type {
  AuthSessionBounds,
  AuthSessionState,
  CredentialDomain,
  CredentialHealth,
  PixivOAuthExchangeResult,
  PixivOAuthStartResult,
  SubscriptionDownloadAttemptRecord,
  SubscriptionInfo,
  SubscriptionIssueRecord,
  SubscriptionProgressEvent,
  SubscriptionRunRecord,
  SubscriptionSiteInfo,
} from '../shared/types/subscriptions';

// ── Grid ────────────────────────────────────────────────────────

/**
 * Query the grid via canonical query_entity_view command.
 * Both loading and reconcile now use the same backend path (ApplicationEngine).
 */
export function queryEntityView(query: EntityViewQuery): Promise<EntityViewPage> {
  return invoke<EntityViewPage>('query_entity_view', query as unknown as Record<string, unknown>);
}

export interface ReconcileResult {
  kind: 'no_change' | 'patch_rows' | 'replace_window' | 'full_refresh_required';
  items?: CanonicalEntityGridItem[];
  page?: EntityViewPage;
}

export function reconcileEntityView(
  query: EntityViewQuery,
  visibleHashes: string[],
  metadataOnly: boolean,
): Promise<ReconcileResult> {
  return invoke<ReconcileResult>('reconcile_entity_view', {
    query,
    visible_hashes: visibleHashes,
    metadata_only: metadataOnly,
  } as unknown as Record<string, unknown>);
}

export function getEntityGridItems(hashes: string[]): Promise<CanonicalEntityGridItem[]> {
  return invoke<CanonicalEntityGridItem[]>('get_entity_grid_items', { entity_hashes: hashes });
}

// ── Inspector / entity details ───────────────────────────────────

export function getEntityDetails(entityHash: string): Promise<CanonicalEntityDetails | null> {
  return invoke<CanonicalEntityDetails | null>('get_entity_details', { entity_hash: entityHash });
}

export interface CollectionSummary {
  id: number;
  name: string;
  image_count: number;
  total_size_bytes: number;
}

export function getCollectionSummary(collectionId: number): Promise<CollectionSummary> {
  return invoke<CollectionSummary>('get_collection_summary', { id: collectionId });
}

// ── Entity mutations ─────────────────────────────────────────────

export function patchMediaEntities(target: EntityTarget, patch: MediaEntityPatch): Promise<unknown> {
  return invoke('patch_media_entities', { target, patch } as unknown as Record<string, unknown>);
}

export function applyEntityTags(
  target: EntityTarget,
  operation: 'add' | 'remove',
  tags: string[],
  provenanceMask?: string | null,
): Promise<unknown> {
  return invoke('apply_entity_tags', {
    target,
    operation,
    tags,
    provenance_mask: provenanceMask ?? null,
  } as unknown as Record<string, unknown>);
}

export function setEntityStatus(target: EntityTarget, status: number): Promise<unknown> {
  return invoke('set_entity_status', { target, status } as unknown as Record<string, unknown>);
}

export function deleteEntities(target: EntityTarget): Promise<unknown> {
  return invoke('delete_entities', { target } as unknown as Record<string, unknown>);
}

export function getSelectionSummary(target: EntityTarget): Promise<SelectionSummary> {
  return invoke<SelectionSummary>('get_selection_summary', { target } as unknown as Record<string, unknown>);
}

export function searchTags(query: string, limit = 50): Promise<CanonicalTagRecord[]> {
  return invoke<CanonicalTagRecord[]>('search_tags', { query, limit });
}

export function getTagsPaginated(params: {
  namespace?: string | null;
  search?: string | null;
  cursor?: string | null;
  limit?: number;
}): Promise<CanonicalTagRecord[]> {
  return invoke<CanonicalTagRecord[]>('get_tags_paginated', params as unknown as Record<string, unknown>);
}

export function getNamespaceSummary(): Promise<CanonicalNamespaceSummary[]> {
  return invoke<CanonicalNamespaceSummary[]>('get_namespace_summary');
}

export function getTagRelations(tagId: number, relationType: 'aliases' | 'implications'): Promise<CanonicalTagRelation[]> {
  return invoke<CanonicalTagRelation[]>('get_tag_relations', {
    tag_id: tagId,
    relation_type: relationType,
  });
}

export function renameTag(tagId: number, newName: string): Promise<unknown> {
  return invoke('rename_tag', { tag_id: tagId, new_name: newName });
}

export function mergeTags(fromTag: string, toTag: string): Promise<unknown> {
  return invoke('merge_tags', { from_tag: fromTag, to_tag: toTag });
}

export function deleteTag(tagId: number): Promise<unknown> {
  return invoke('delete_tag', { tag_id: tagId });
}

export function manageTagAlias(from: string, to?: string | null): Promise<void> {
  return invoke<void>('manage_tag_alias', { from, to: to ?? null });
}

export function manageTagImplication(
  child: string,
  parent: string,
  action: 'add' | 'remove',
): Promise<void> {
  return invoke<void>('manage_tag_implication', { child, parent, action });
}

export function setTagSiteMask(tagId: number, siteMask: string): Promise<void> {
  return invoke<void>('set_tag_site_mask', { tag_id: tagId, site_mask: siteMask });
}

// ── Collections ─────────────────────────────────────────────────

export function createCollection(name: string): Promise<number> {
  return invoke<number>('create_collection', { name });
}

export function addCollectionMembers(collectionId: number, hashes: string[]): Promise<number> {
  return invoke<number>('add_collection_members', { id: collectionId, hashes });
}

export function removeCollectionMembers(collectionId: number, hashes: string[]): Promise<number> {
  return invoke<number>('remove_collection_members', { id: collectionId, hashes });
}

export function reorderCollectionMembers(collectionId: number, orderedHashes: string[]): Promise<void> {
  return invoke<void>('reorder_collection_members', { id: collectionId, hashes: orderedHashes });
}

/** Split/delete a collection — frees all members as standalone singles. */
export function deleteCollection(collectionId: number): Promise<void> {
  return invoke<void>('delete_collection', { id: collectionId });
}

export function listCollectionMemberHashes(collectionId: number): Promise<string[]> {
  return invoke<string[]>('list_collection_member_hashes', { id: collectionId });
}

// ── Sidebar ──────────────────────────────────────────────────────

export function getSidebarTree(): Promise<SidebarTreeResponse> {
  return invoke<SidebarTreeResponse>('get_sidebar_tree');
}

export function openExternalUrl(url: string): Promise<void> {
  return invoke<void>('open_external_url', { url });
}

export function openSettingsWindow(): Promise<void> {
  return invoke<void>('open_settings_window');
}

export function reorderSidebarNodes(moves: [string, number][]): Promise<void> {
  return invoke<void>('reorder_sidebar_nodes', { moves });
}

// ── Folders ──────────────────────────────────────────────────────

export function createFolder(params: {
  name: string;
  parent_id?: number | null;
  icon?: string;
  color?: string;
}): Promise<unknown> {
  return invoke('create_folder', params);
}

export function deleteFolder(folderId: number): Promise<void> {
  return invoke<void>('delete_folder', { folder_id: folderId });
}

export function removeEntitiesFromFolder(folderId: number, target: EntityTarget): Promise<void> {
  return invoke<void>('remove_entities_from_folder', { folder_id: folderId, target });
}

export function renameFolder(folderId: number, name: string): Promise<void> {
  return invoke<void>('update_folder', { folder_id: folderId, name });
}

export function updateFolder(folderId: number, patch: {
  name?: string;
  icon?: string | null;
  color?: string | null;
  notes?: string | null;
}): Promise<void> {
  return invoke<void>('update_folder', { folder_id: folderId, ...patch });
}

export function moveFolder(
  folderId: number,
  newParentId: number | null,
  siblingOrder: [number, number][],
): Promise<void> {
  return invoke<void>('move_folder', {
    folder_id: folderId,
    new_parent_id: newParentId,
    sibling_order: siblingOrder,
  });
}

export function updateFolderMembership(
  target: EntityTarget,
  folderId: number,
  operation: 'add' | 'remove',
): Promise<unknown> {
  return invoke('update_folder_membership', {
    target,
    folder_id: folderId,
    operation,
  } as unknown as Record<string, unknown>);
}

export function setFolderWatchConfig(folderId: number, config: {
  watch_path: string;
  watch_enabled: boolean;
  watch_subfolders: boolean;
  watch_import_status_mode: string;
}): Promise<void> {
  return invoke<void>('set_folder_watch_config', { folder_id: folderId, ...config });
}

export function clearFolderWatchConfig(folderId: number): Promise<void> {
  return invoke<void>('clear_folder_watch_config', { folder_id: folderId });
}

// ── Smart folders ────────────────────────────────────────────────

export function createSmartFolder(params: {
  folder: SmartFolderCommandPayload;
}): Promise<unknown> {
  return invoke('create_smart_folder', params as unknown as Record<string, unknown>);
}

export function deleteSmartFolder(id: string): Promise<void> {
  return invoke<void>('delete_smart_folder', { id });
}

// NOTE: update_smart_folder requires a full SmartFolder struct.
// No partial patch command exists.

export function updateSmartFolder(params: {
  id: string;
  folder: SmartFolderCommandPayload;
}): Promise<void> {
  return invoke<void>('update_smart_folder', params);
}

export function moveSmartFolder(
  smartFolderId: number,
  newParentId: number | null,
  siblingOrder: [number, number][],
): Promise<void> {
  return invoke<void>('move_smart_folder', {
    smart_folder_id: smartFolderId,
    new_parent_id: newParentId,
    sibling_order: siblingOrder,
  });
}

// ── Subscriptions ───────────────────────────────────────────────

export function getSubscriptionSites(): Promise<SubscriptionSiteInfo[]> {
  return invoke<SubscriptionSiteInfo[]>('get_sites');
}

export function getSubscriptions(): Promise<SubscriptionInfo[]> {
  return invoke<SubscriptionInfo[]>('get_subscriptions');
}

export function createGroup(name: string, schedule?: string | null): Promise<unknown> {
  return invoke('create_group', {
    name,
    schedule: schedule ?? null,
  } as unknown as Record<string, unknown>);
}

export function createSubscription(params: {
  name: string;
  group_id?: number | null;
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

export function setSubscriptionAutoCollections(id: string, autoCollections: boolean): Promise<void> {
  return invoke<void>('set_subscription_auto_collections', {
    id,
    auto_collections: autoCollections,
  });
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
  site_id: string;
  post_id: string;
}): Promise<void> {
  return invoke<void>('retry_subscription_failed_post', input as unknown as Record<string, unknown>);
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
): Promise<SubscriptionIssueRecord[]> {
  return invoke<SubscriptionIssueRecord[]>('list_subscription_issues', {
    subscription_id: subscriptionId,
    query_id: queryId ?? null,
    limit,
  });
}

export function listSubscriptionDownloadAttempts(
  subscriptionId: string,
  queryId?: string | null,
  limit = 50,
): Promise<SubscriptionDownloadAttemptRecord[]> {
  return invoke<SubscriptionDownloadAttemptRecord[]>('list_subscription_download_attempts', {
    subscription_id: subscriptionId,
    query_id: queryId ?? null,
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

export function setAuthSessionBounds(bounds: AuthSessionBounds): Promise<void> {
  return invoke<void>('auth_session_set_bounds', bounds as unknown as Record<string, unknown>);
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

// ── Folder cover ────────────────────────────────────────────────

export function getFolderCoverHash(folderId: number): Promise<string | null> {
  return invoke<string | null>('get_folder_cover_hash', { folder_id: folderId });
}

// ── Import / Export ─────────────────────────────────────────────

export function importFiles(paths: string[], params?: {
  tag_strings?: string[];
  source_urls?: string[];
  initial_status?: number;
  parent_folder_id?: number | null;
}): Promise<unknown> {
  return invoke('import_files', { paths, ...params } as unknown as Record<string, unknown>);
}

export function importFolder(path: string, params?: {
  preserve_structure?: boolean;
  parent_folder_id?: number | null;
  initial_status?: number;
}): Promise<unknown> {
  return invoke('import_folder', {
    path,
    preserve_structure: params?.preserve_structure ?? true,
    parent_folder_id: params?.parent_folder_id ?? null,
    initial_status: params?.initial_status ?? 1,
  } as unknown as Record<string, unknown>);
}

export function exportMedia(target: EntityTarget, config: {
  output_dir: string;
  format?: string | null;
  quality?: number | null;
  width?: number | null;
  height?: number | null;
  keep_aspect?: boolean;
}): Promise<unknown> {
  return invoke('export_media', { target, ...config } as unknown as Record<string, unknown>);
}

// ── Folder operations ───────────────────────────────────────────

export function reorderFolderItems(folderId: number, params: {
  sort_by?: string;
  direction?: string;
  moves?: Array<{ hash: string; before_hash?: string | null; after_hash?: string | null }>;
  hashes?: string[];
}): Promise<void> {
  return invoke<void>('reorder_folder_items', { folder_id: folderId, ...params } as unknown as Record<string, unknown>);
}

/** New engine: reorder folder members by entity_id + position_rank. */
export function reorderFolderMembers(folderId: number, moves: [number, number][]): Promise<void> {
  return invoke<void>('reorder_folder_members', { folder_id: folderId, moves });
}

// ── AI Tagger ───────────────────────────────────────────────────

export interface AiTaggerStatus {
  models: Array<{
    model: string;
    downloaded: boolean;
    enabled: boolean;
    size_bytes: number | null;
  }>;
  gpu_backend: string | null;
}

export function aiTaggerStatus(): Promise<AiTaggerStatus> {
  return invoke<AiTaggerStatus>('ai_tagger_status');
}

export function aiTaggerDownloadModel(model: string): Promise<void> {
  return invoke<void>('ai_tagger_download_model', { model });
}

export function aiTaggerDeleteModel(model: string): Promise<void> {
  return invoke<void>('ai_tagger_delete_model', { model });
}

export interface TagPrediction {
  namespace: string;
  tag: string;
  confidence: number;
}

export interface FilePrediction {
  hash: string;
  tags: TagPrediction[];
  error?: string | null;
}

export function aiTagPredict(hashes: string[], models?: string[]): Promise<{ predictions: FilePrediction[] }> {
  return invoke<{ predictions: FilePrediction[] }>('ai_tag_predict', {
    hashes,
    models: models ?? null,
  } as unknown as Record<string, unknown>);
}

export function aiTagApply(hashes: string[], tags: string[]): Promise<{ applied_count: number }> {
  return invoke<{ applied_count: number }>('ai_tag_apply', { hashes, tags });
}

// ── Shell operations ────────────────────────────────────────────

export interface EntityAssetResult {
  role: string;
  available: boolean;
  url?: string | null;
  mime_type?: string | null;
  path?: string | null;
  source_entity_hash?: string | null;
}

export function resolveEntityAsset(hash: string, role: string): Promise<EntityAssetResult> {
  return invoke<EntityAssetResult>('resolve_entity_asset', { entity_hash: hash, role });
}

export function resolveFilePath(hash: string): Promise<string | null> {
  return invoke<string | null>('resolve_file_path', { hash });
}

export function openDetailWindow(input: {
  hash: string;
  width?: number | null;
  height?: number | null;
}): Promise<void> {
  return invoke<void>('open_in_new_window', input as unknown as Record<string, unknown>);
}

export function shellShowInFolder(path: string): void {
  (window as any).picto?.shell?.showInFolder(path);
}

export function shellOpenPath(path: string): void {
  (window as any).picto?.shell?.openPath(path);
}

export function clipboardWriteText(text: string): void {
  (window as any).picto?.clipboard?.writeText(text);
}

export function clipboardCopyFile(path: string): void {
  (window as any).picto?.clipboard?.copyFile(path);
}

export function regenerateThumbnailsBatch(hashes: string[]): Promise<{ total: number; regenerated: number; errors: number }> {
  return invoke('regenerate_thumbnails_batch', { hashes } as unknown as Record<string, unknown>);
}

// ── View preferences ────────────────────────────────────────────

export interface ViewPrefsDto {
  scope_key: string;
  sort_field: string | null;
  sort_order: string | null;
  view_mode: string | null;
  target_size: number | null;
  show_name: boolean | null;
  show_resolution: boolean | null;
  show_extension: boolean | null;
  show_label: boolean | null;
  thumbnail_fit: string | null;
}

export interface ViewPrefsPatch {
  sort_field?: string | null;
  sort_order?: string | null;
  view_mode?: string | null;
  target_size?: number | null;
  show_name?: boolean | null;
  show_resolution?: boolean | null;
  show_extension?: boolean | null;
  show_label?: boolean | null;
  thumbnail_fit?: string | null;
  show_subfolders?: boolean | null;
}

export function setZoomFactor(factor: number): Promise<void> {
  return invoke<void>('set_zoom_factor', { factor });
}

export function getViewPrefs(scopeKey: string): Promise<ViewPrefsDto> {
  return invoke<ViewPrefsDto>('get_view_prefs', { scope_key: scopeKey });
}

export function setViewPrefs(scopeKey: string, patch: ViewPrefsPatch): Promise<ViewPrefsDto> {
  return invoke<ViewPrefsDto>('set_view_prefs', { scope_key: scopeKey, patch });
}

// ── App settings (JSON file) ────────────────────────────────────

export interface AppSettings {
  gridTargetSize: number;
  gridViewMode: string;
  inspectorWidth: number;
  colorScheme: string;
  gridSortField: string;
  gridSortOrder: string;
  zoomFactor: number | null;
  showTreeGuides: boolean;
  [key: string]: unknown;
}

export function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>('get_settings', {});
}

export function saveSettings(settings: Partial<AppSettings>): Promise<void> {
  // get_settings returns full object, save_settings expects full object.
  // Merge patch into current settings.
  return getSettings().then((current) =>
    invoke<void>('save_settings', { ...current, ...settings }),
  );
}
