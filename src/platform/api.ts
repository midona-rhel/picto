// ─── Re-exports from sub-modules (preserves all existing import paths) ───
export { invoke, listen, emit, emitTo } from './ipc';
export type { UnlistenFn } from './ipc';
export { PhysicalSize, getCurrentWindow, setTheme, getCurrentWebview } from './window';
export { open, save, writeText, copyFileToClipboard, copyImageToClipboard, reverseImageSearch, libraryHost } from './nativeIntegration';
export type { ReverseImageEngine, LibraryConfig } from './nativeIntegration';
export { load } from './store';
export type { Store } from './store';

// ─── Typed event + command dispatch ─────────────────────────────────────────

import { invoke, listen } from './ipc';
import type { UnlistenFn } from './ipc';
import type { RuntimeSnapshot } from '../shared/types/backendState';
import type { CoreRuntimeEventPayloadMap } from '../shared/types/api/events';

import type {
  EntityAllMetadata,
  EntityDetails,
  EntityMetadataBatchResponse,
  EntitySlim,
  GridOutlineResponse,
  GridPageSlimResponse, GridPageSlimQuery,
  EnsureThumbnailResponse, ReanalyzeFileColorsResponse,
  TagDisplay, TagSearchResult, TagTuple, TagRecord,
  NamespaceSummary, TagRelation,
  RenameTagResult, DeleteTagResult,
  SelectionQuerySpec, SelectionSummary,
  Folder, FolderMembership, FolderReorderMove,
  SmartFolder, SmartFolderPredicate, SmartFolderIpcInput,
  SidebarTreeResponse,
  ScanDuplicatesResult, DuplicatePairsResponse, DuplicateSettings,
  SmartMergeResult, ResolveDuplicateAction,
  SubscriptionInfo, SubscriptionQueryInfo, SubscriptionGroupInfo,
  SubscriptionProgressEvent,
  SubscriptionSiteInfo, SiteMetadataSchema, SiteMetadataValidationResult,
  CredentialDomain, CredentialType, CredentialHealth,
  AppSettings, AiTaggerStatus, AiTagPredictOutput,
  CollectionInfo, CollectionSummary, CompanionNamespaceValue,
  ViewPrefsDto, ViewPrefsPatch,
  FileStats, PerfSnapshot, PerfSloResult,
  LibraryInfo,
} from '../shared/types/api';

export function listenRuntimeEvent<K extends keyof CoreRuntimeEventPayloadMap>(
  eventName: K,
  handler: (payload: CoreRuntimeEventPayloadMap[K]) => void,
): Promise<UnlistenFn> {
  return listen<CoreRuntimeEventPayloadMap[K]>(eventName, (e) => handler(e.payload));
}

export function getRuntimeSnapshot(): Promise<RuntimeSnapshot> {
  return api.runtime.getSnapshot();
}

export function openExternalUrl(url: string): Promise<void> {
  return api.os.openExternalUrl(url);
}

export function openSettingsWindow(): Promise<void> {
  return api.os.openSettingsWindow();
}

export function openSubscriptionsWindow(): Promise<void> {
  return api.os.openSubscriptionsWindow();
}

export function openLibraryManager(): Promise<void> {
  return invoke<void>('open_library_manager');
}

// ─── Typed command dispatch (PBI-234) ──────────────────────────────────────
//
// Generated types live in types/generated/commands/ (via ts-rs from Rust).
// invokeTyped() provides compile-time checked command names and argument types.

import type { TypedCommandMap } from '../shared/types/generated/commands';
import type { ImportBatchResult } from '../shared/types/generated/commands';
import type { ExportMediaInput, ExportMediaResult } from '../shared/types/generated/commands';

type HasInput<K extends keyof TypedCommandMap> =
  TypedCommandMap[K]['input'] extends Record<string, never> ? false : true;

export function invokeTyped<K extends keyof TypedCommandMap>(
  command: K,
  ...args: HasInput<K> extends true ? [TypedCommandMap[K]['input']] : []
): Promise<TypedCommandMap[K]['output']> {
  return invoke(command, (args[0] ?? {}) as Record<string, unknown>);
}

export type { TypedCommandMap } from '../shared/types/generated/commands';

export { api as desktopTypedApi };

/** Normalize backend SmartFolder shape (smart_folder_id → id, predicate_json → predicate). */
function normalizeSmartFolder(r: Record<string, unknown>): SmartFolder {
  return {
    id: String(r.smart_folder_id ?? r.id ?? ''),
    name: String(r.name ?? ''),
    parent_id: typeof r.parent_id === 'number' ? r.parent_id : r.parent_id == null ? null : Number(r.parent_id),
    icon: (r.icon as string | null) ?? undefined,
    color: (r.color as string | null) ?? undefined,
    predicate: r.predicate_json
      ? JSON.parse(String(r.predicate_json))
      : (r.predicate as SmartFolderPredicate) ?? { groups: [] },
    sort_field: (r.sort_field as string | null) ?? undefined,
    sort_order: (r.sort_order as string | null) ?? undefined,
    created_at: (r.created_at as string | null) ?? undefined,
    updated_at: (r.updated_at as string | null) ?? undefined,
  };
}

const entityApi = {
  get: (hash: string) =>
    invokeTyped('get_entity', { hash }) as Promise<EntityDetails | null>,
  getAllMetadata: (hash: string) =>
    invokeTyped('get_media_entity_metadata', { hash }) as Promise<EntityAllMetadata>,
  setStatus: (hash: string, status: string) =>
    invokeTyped('set_entity_status', { hash, status } as never) as unknown as Promise<void>,
  setStatusSelection: (selection: SelectionQuerySpec, status: string) =>
    invokeTyped('set_entity_status', { selection, status } as never),
  deleteMany: (hashes: string[]) =>
    invokeTyped('delete_entities', { hashes } as never),
  deleteSelection: (selection: SelectionQuerySpec) =>
    invokeTyped('delete_entities', { selection } as never),
  updateRating: (hash: string, rating: number | null) =>
    invokeTyped('update_media_entity_metadata', { hash, rating } as never) as unknown as Promise<void>,
  setName: (hash: string, name: string | null) =>
    invokeTyped('update_media_entity_metadata', { hash, name } as never) as unknown as Promise<void>,
  setSourceUrls: (hash: string, urls: string[]) =>
    invokeTyped('update_media_entity_metadata', { hash, source_urls: urls } as never) as unknown as Promise<void>,
  setNotes: (hash: string, notes: Record<string, string>) =>
    invokeTyped('update_media_entity_metadata', { hash, notes } as never) as unknown as Promise<void>,
  resolvePath: (hash: string) =>
    invokeTyped('resolve_file_path', { hash }),
  resolveThumbnailPath: (hash: string) =>
    invokeTyped('resolve_thumbnail_path', { hash }),
  openDefault: (hash: string) =>
    invokeTyped('open_file_default', { hash }) as unknown as Promise<void>,
  revealInFolder: (hash: string) =>
    invokeTyped('reveal_in_folder', { hash }) as unknown as Promise<void>,
  openInNewWindow: (hash: string, width?: number | null, height?: number | null) =>
    invokeTyped('open_in_new_window', { hash, width: width ?? null, height: height ?? null }) as unknown as Promise<void>,
  ensureThumbnail: (hash: string) =>
    invokeTyped('ensure_thumbnail', { hash }) as Promise<EnsureThumbnailResponse>,
  regenerateThumbnail: (hash: string) =>
    invokeTyped('regenerate_thumbnail', { hash }) as Promise<EnsureThumbnailResponse>,
  reanalyzeColors: (hash: string) =>
    invokeTyped('reanalyze_file_colors', { hash }) as Promise<ReanalyzeFileColorsResponse>,
  regenerateThumbnailsBatch: (hashes: string[]) =>
    invokeTyped('regenerate_thumbnails_batch', { hashes }) as Promise<{ total: number; regenerated: number; errors: number }>,
};

/**
 * Typed API surface — single place where all backend command strings live.
 * Every invoke() in the codebase should route through here.
 */
export const api = {
  grid: {
    getPageSlim: (query: GridPageSlimQuery) =>
      invokeTyped('get_grid_page_slim', { query } as never) as Promise<GridPageSlimResponse>,
    getOutline: (query: GridPageSlimQuery) =>
      invoke('get_grid_outline', { query }) as Promise<GridOutlineResponse>,
    getEntitiesMetadataBatch: (hashes: string[]) =>
      invokeTyped('get_entities_metadata_batch', { hashes }) as Promise<EntityMetadataBatchResponse>,
  },

  entity: entityApi,
  files: entityApi,
  file: entityApi,

  import: {
    files: (paths: string[], tagStrings?: string[], sourceUrls?: string[], initialStatus?: number) =>
      invokeTyped('import_files', { paths, tag_strings: tagStrings, source_urls: sourceUrls, initial_status: initialStatus } as never) as unknown as Promise<ImportBatchResult>,
    folder: (path: string, preserveStructure: boolean, parentFolderId?: number | null, initialStatus?: number) =>
      invokeTyped('import_folder', {
        path,
        preserve_structure: preserveStructure,
        parent_folder_id: parentFolderId ?? null,
        initial_status: initialStatus,
      } as never) as unknown as Promise<ImportBatchResult>,
  },

  export: {
    file: (hash: string, destPath: string) =>
      invokeTyped('export_file', { hash, dest_path: destPath }) as Promise<null>,
    run: (input: ExportMediaInput) =>
      invokeTyped('export_media', input) as Promise<ExportMediaResult>,
  },

  tags: {
    search: (query: string, limit?: number) =>
      invokeTyped('search_tags', { query, limit } as never) as Promise<TagSearchResult[]>,
    getAll: () =>
      invokeTyped('get_all_tags_with_counts') as Promise<TagTuple[]>,
    getForFile: (hash: string) =>
      invokeTyped('get_entity_tags', { hash }) as Promise<TagDisplay[]>,
    add: (hashes: string[], tagStrings: string[]) =>
      invokeTyped('add_tags', { hashes, tag_strings: tagStrings }) as unknown as Promise<void>,
    remove: (hashes: string[], tagStrings: string[]) =>
      invokeTyped('remove_tags', { hashes, tag_strings: tagStrings }) as unknown as Promise<void>,
    findFilesByTags: (tagStrings: string[], limit?: number, offset?: number) =>
      invokeTyped('find_files_by_tags', { tag_strings: tagStrings, limit, offset } as never) as Promise<string[]>,
    getPaginated: (params: { namespace?: string; search?: string; cursor?: string; limit?: number }) =>
      invokeTyped('get_tags_paginated', params as never) as Promise<TagRecord[]>,
    getNamespaceSummary: () =>
      invokeTyped('get_namespace_summary') as Promise<NamespaceSummary[]>,
    manageAlias: (from: string, to?: string) =>
      invokeTyped('manage_tag_alias', { from, to: to ?? null }) as unknown as Promise<void>,
    getRelations: (tagId: number, relationType: 'aliases' | 'implications') =>
      invokeTyped('get_tag_relations', { tag_id: tagId, relation_type: relationType }) as Promise<TagRelation[]>,
    manageImplication: (child: string, parent: string, action: 'add' | 'remove') =>
      invokeTyped('manage_tag_implication', { child, parent, action }) as unknown as Promise<void>,
    merge: (fromTag: string, toTag: string) =>
      invokeTyped('merge_tags', { from_tag: fromTag, to_tag: toTag }) as unknown as Promise<void>,
    rename: (tagId: number, newName: string) =>
      invokeTyped('rename_tag', { tag_id: tagId, new_name: newName }) as Promise<RenameTagResult>,
    delete: (tagId: number) =>
      invokeTyped('delete_tag', { tag_id: tagId }) as Promise<DeleteTagResult>,
    searchPaged: (query: string, limit: number, offset: number) =>
      invokeTyped('search_tags', { query, limit, offset } as never) as Promise<[string, string, number][]>,
  },

  selection: {
    getSummary: (selection: SelectionQuerySpec) =>
      invokeTyped('get_selection_summary', { selection } as never) as Promise<SelectionSummary>,
    resolveHashes: (selection: SelectionQuerySpec) =>
      invoke<string[]>('resolve_selection_hashes', { selection }),
    addTags: (selection: SelectionQuerySpec, tagStrings: string[]) =>
      invokeTyped('add_tags_selection', { selection, tag_strings: tagStrings } as never),
    removeTags: (selection: SelectionQuerySpec, tagStrings: string[]) =>
      invokeTyped('remove_tags_selection', { selection, tag_strings: tagStrings } as never),
    updateRating: (selection: SelectionQuerySpec, rating: number | null) =>
      invokeTyped('update_selection_metadata', { selection, rating } as never),
    setNotes: (selection: SelectionQuerySpec, notes: Record<string, string>) =>
      invokeTyped('update_selection_metadata', { selection, notes } as never),
    setSourceUrls: (selection: SelectionQuerySpec, urls: string[]) =>
      invokeTyped('update_selection_metadata', { selection, source_urls: urls } as never),
  },

  folders: {
    list: () =>
      invokeTyped('list_folders') as Promise<Folder[]>,
    create: (params: { name: string; parent_id?: number | null; icon?: string; color?: string }) =>
      invokeTyped('create_folder', params as never) as Promise<Folder>,
    update: (params: { folder_id: number; name?: string; icon?: string; color?: string; auto_tags?: string[] }) =>
      invokeTyped('update_folder', params as never) as unknown as Promise<void>,
    setWatchConfig: (params: {
      folder_id: number;
      watch_path: string;
      watch_enabled?: boolean;
      watch_subfolders: boolean;
      watch_import_status_mode: 'inherit' | 'inbox' | 'active';
      import_existing_now: boolean;
    }) =>
      invokeTyped('set_folder_watch_config', params as never) as unknown as Promise<void>,
    clearWatchConfig: (folderId: number) =>
      invokeTyped('clear_folder_watch_config', { folder_id: folderId } as never) as unknown as Promise<void>,
    delete: (folderId: number) =>
      invokeTyped('delete_folder', { folder_id: folderId }) as unknown as Promise<void>,
    updateParent: (folderId: number, newParentId?: number | null) =>
      invokeTyped('update_folder_parent', { folder_id: folderId, new_parent_id: newParentId } as never) as unknown as Promise<void>,
    // PBI-057: Atomic move_folder — reparent + reorder in one transaction.
    moveFolder: (folderId: number, newParentId: number | null, siblingOrder: [number, number][]) =>
      invokeTyped('move_folder', { folder_id: folderId, new_parent_id: newParentId, sibling_order: siblingOrder }) as unknown as Promise<void>,
    addFiles: (folderId: number, hashes: string[], selection?: SelectionQuerySpec) =>
      invokeTyped('add_entities_to_folder', { folder_id: folderId, hashes, selection: selection ?? null } as never) as unknown as Promise<number>,
    removeFiles: (folderId: number, hashes: string[], selection?: SelectionQuerySpec) =>
      invokeTyped('remove_entities_from_folder', { folder_id: folderId, hashes, selection: selection ?? null } as never) as unknown as Promise<number>,
    getFiles: (folderId: number) =>
      invokeTyped('get_folder_files', { folder_id: folderId }),
    getCoverHash: (folderId: number) =>
      invokeTyped('get_folder_cover_hash', { folder_id: folderId }),
    getFileFolders: (hash: string) =>
      invokeTyped('get_entity_folders_by_hash', { hash }) as Promise<FolderMembership[]>,
    getEntityFolders: (entityId: number) =>
      invokeTyped('get_entity_folders', { entity_id: entityId }) as Promise<FolderMembership[]>,
    reorder: (moves: [number, number][]) =>
      invokeTyped('reorder_folders', { moves }) as unknown as Promise<void>,
    reorderItems: (folderId: number, moves: FolderReorderMove[]) =>
      invokeTyped('reorder_folder_items', { folder_id: folderId, moves } as never) as unknown as Promise<void>,
    sortItems: (folderId: number, sortBy: string, direction: string, hashes?: string[]) =>
      invokeTyped('reorder_folder_items', { folder_id: folderId, sort_by: sortBy, direction, hashes } as never) as unknown as Promise<void>,
    reverseItems: (folderId: number, hashes?: string[]) =>
      invokeTyped('reorder_folder_items', { folder_id: folderId, reverse: true, hashes } as never) as unknown as Promise<void>,
  },

  smartFolders: {
    list: async (): Promise<SmartFolder[]> => {
      const raw = await invokeTyped('list_smart_folders') as Array<Record<string, unknown>>;
      return raw.map(normalizeSmartFolder);
    },
    create: async (folder: SmartFolderIpcInput): Promise<SmartFolder> => {
      const raw = await invokeTyped('create_smart_folder', { folder } as never) as Record<string, unknown>;
      return normalizeSmartFolder(raw);
    },
    update: async (id: string, folder: SmartFolderIpcInput): Promise<SmartFolder> => {
      const raw = await invokeTyped('update_smart_folder', { id, folder } as never) as Record<string, unknown>;
      return normalizeSmartFolder(raw);
    },
    delete: (id: string) =>
      invokeTyped('delete_smart_folder', { id }) as unknown as Promise<void>,
    count: (predicate: SmartFolderPredicate) =>
      invokeTyped('count_smart_folder', { predicate } as never) as Promise<number>,
    reorder: (parentId: number | null, moves: [number, number][]) =>
      invokeTyped('reorder_smart_folders', { parent_id: parentId, moves } as never) as unknown as Promise<void>,
    move: (smartFolderId: number, newParentId: number | null, siblingOrder: [number, number][]) =>
      invokeTyped('move_smart_folder', {
        smart_folder_id: smartFolderId,
        new_parent_id: newParentId,
        sibling_order: siblingOrder,
      } as never) as unknown as Promise<void>,
  },

  sidebar: {
    getTree: () =>
      invokeTyped('get_sidebar_tree') as Promise<SidebarTreeResponse>,
    reorderNodes: (moves: [string, number][]) =>
      invokeTyped('reorder_sidebar_nodes', { moves }) as unknown as Promise<void>,
  },

  duplicates: {
    getPairs: (cursor?: string | null, limit?: number, status?: string) =>
      invokeTyped('get_duplicate_pairs', {
        cursor: cursor ?? null,
        limit: limit ?? 50,
        status: status ?? null,
      } as never) as Promise<DuplicatePairsResponse>,
    resolvePair: (action: ResolveDuplicateAction, hashA: string, hashB: string) =>
      invokeTyped('resolve_duplicate_pair', {
        action,
        hash_a: hashA,
        hash_b: hashB,
      } as never) as Promise<SmartMergeResult | Record<string, string>>,
    getCount: () =>
      invokeTyped('get_duplicate_count') as Promise<{ count: number }>,
    scan: () =>
      invokeTyped('scan_duplicates', { threshold: null } as never) as Promise<ScanDuplicatesResult>,
    getSettings: () =>
      invokeTyped('get_duplicate_settings') as Promise<DuplicateSettings>,
    updateSettings: (settings: Partial<DuplicateSettings>) =>
      invokeTyped('update_duplicate_settings', settings as never) as Promise<{ ok: boolean }>,
    findSimilar: (hash: string) =>
      invoke<{ source_hash: string; items: Array<{ hash: string; distance: number }> }>('find_similar', { hash }),
  },

  subscriptions: {
    list: () =>
      invokeTyped('get_subscriptions') as Promise<SubscriptionInfo[]>,
    create: (params: {
      name: string;
      site_id: string;
      queries: string[];
      group_id?: number;
      initial_post_limit?: number;
      periodic_post_limit?: number;
    }) =>
      invokeTyped('create_subscription', params as never) as Promise<SubscriptionInfo>,
    delete: (id: string, deleteFiles?: boolean) =>
      invokeTyped('delete_subscription', { id, delete_files: deleteFiles ?? null } as never) as Promise<number>,
    rename: (id: string, name: string) =>
      invokeTyped('rename_subscription', { id, name }) as unknown as Promise<void>,
    pause: (id: string, paused: boolean) =>
      invokeTyped('pause_subscription', { id, paused }) as unknown as Promise<void>,
    run: (id: string) =>
      invokeTyped('run_subscription', { id }) as unknown as Promise<void>,
    stop: (id: string) =>
      invokeTyped('stop_subscription', { id }) as unknown as Promise<void>,
    reset: (id: string) =>
      invokeTyped('reset_subscription', { id }) as unknown as Promise<void>,
    getRunning: () =>
      invokeTyped('get_running_subscriptions') as Promise<string[]>,
    getRunningProgress: () =>
      invokeTyped('get_running_subscription_progress') as Promise<SubscriptionProgressEvent[]>,
    addQuery: (subscriptionId: string, queryText: string) =>
      invokeTyped('add_subscription_query', { subscription_id: subscriptionId, query_text: queryText }) as Promise<SubscriptionQueryInfo>,
    deleteQuery: (id: string) =>
      invokeTyped('delete_subscription_query', { id }) as unknown as Promise<void>,
    editQuery: (id: number, queryText: string, displayName?: string | null) =>
      invoke<void>('edit_subscription_query', { id, query_text: queryText, display_name: displayName ?? null }),
    pauseQuery: (id: string, paused: boolean) =>
      invokeTyped('pause_subscription_query', { id, paused }) as unknown as Promise<void>,
    setAutoCollections: (subscriptionId: string, autoCollections: boolean) =>
      invoke<void>('set_subscription_auto_collections', { id: subscriptionId, auto_collections: autoCollections }),
    runQuery: (subscriptionId: string, queryId: string) =>
      invokeTyped('run_subscription_query', { subscription_id: subscriptionId, query_id: queryId }) as unknown as Promise<void>,
    getSites: () =>
      invokeTyped('get_sites') as Promise<SubscriptionSiteInfo[]>,
    getSiteMetadataSchema: (siteId: string) =>
      invokeTyped('get_site_metadata_schema', { site_id: siteId }) as Promise<SiteMetadataSchema>,
    validateSiteMetadata: (params: {
      site_id: string;
      sample_url?: string;
      sample_metadata_json?: Record<string, unknown> | null;
    }) =>
      invokeTyped('validate_site_metadata', params as never) as Promise<SiteMetadataValidationResult>,
    listCredentials: () =>
      invokeTyped('list_credentials') as Promise<CredentialDomain[]>,
    listCredentialHealth: () =>
      invokeTyped('list_credential_health') as Promise<CredentialHealth[]>,
    setCredential: (params: {
      site_category: string;
      credential_type: CredentialType;
      display_name?: string | null;
      username?: string | null;
      password?: string | null;
      cookies?: Record<string, string> | null;
      oauth_token?: string | null;
    }) =>
      invokeTyped('set_credential', params as never) as unknown as Promise<void>,
    deleteCredential: (siteCategory: string) =>
      invokeTyped('delete_credential', { site_category: siteCategory }) as unknown as Promise<void>,
    pixivOAuthStart: () =>
      invoke<{ login_url: string; code_verifier: string }>('pixiv_oauth_start'),
    pixivOAuthPopup: (loginUrl: string) =>
      invoke<{ code: string; phpsessid: string | null }>('pixiv_oauth_popup', { login_url: loginUrl }),
    pixivOAuthExchange: (code: string, codeVerifier: string, phpsessid?: string | null) =>
      invoke<{ ok: boolean }>('pixiv_oauth_exchange', { code, code_verifier: codeVerifier, phpsessid: phpsessid ?? null }),
  },

  groups: {
    list: () =>
      invokeTyped('get_groups') as Promise<SubscriptionGroupInfo[]>,
    create: (name: string, schedule?: string) =>
      invokeTyped('create_group', { name, schedule: schedule ?? null } as never) as Promise<SubscriptionGroupInfo>,
    delete: (id: string, deleteFiles?: boolean) =>
      invokeTyped('delete_group', { id, delete_files: deleteFiles ?? null } as never) as unknown as Promise<void>,
    rename: (id: string, name: string) =>
      invokeTyped('rename_group', { id, name }) as unknown as Promise<void>,
    setSchedule: (id: string, schedule: string) =>
      invokeTyped('set_group_schedule', { id, schedule }) as unknown as Promise<void>,
    run: (id: string) =>
      invokeTyped('run_group', { id }) as unknown as Promise<void>,
    stop: (id: string) =>
      invokeTyped('stop_group', { id }) as unknown as Promise<void>,
  },

  settings: {
    get: () =>
      invokeTyped('get_settings') as Promise<AppSettings>,
    save: (settings: Partial<AppSettings>) =>
      invokeTyped('save_settings', settings as never) as unknown as Promise<void>,
    getViewPrefs: (scopeKey?: string) =>
      invokeTyped('get_view_prefs', { scope_key: scopeKey ?? null } as never) as Promise<ViewPrefsDto | null>,
    setViewPrefs: (scopeKey: string | undefined, patch: ViewPrefsPatch) =>
      invokeTyped('set_view_prefs', { scope_key: scopeKey ?? null, patch } as never) as Promise<ViewPrefsDto>,
    setZoomFactor: (factor: number) =>
      invokeTyped('set_zoom_factor', { factor }) as unknown as Promise<void>,
    getZoomFactor: () =>
      invokeTyped('get_zoom_factor') as Promise<number>,
  },

  aiTagger: {
    status: () =>
      invoke<AiTaggerStatus>('ai_tagger_status', {}),
    downloadModel: (model: string) =>
      invoke<void>('ai_tagger_download_model', { model }),
    deleteModel: (model: string) =>
      invoke<void>('ai_tagger_delete_model', { model }),
    predict: (hashes: string[], models?: string[]) =>
      invoke<AiTagPredictOutput>('ai_tag_predict', { hashes, models: models ?? null }),
    apply: (hashes: string[], tags: string[]) =>
      invoke<number>('ai_tag_apply', { hashes, tags }),
  },

  stats: {
    getImageStorageStats: () =>
      invokeTyped('get_storage_stats') as Promise<FileStats>,
    getPerfSnapshot: () =>
      invokeTyped('get_perf_snapshot') as Promise<PerfSnapshot>,
    checkPerfSlo: () =>
      invokeTyped('check_perf_slo') as Promise<PerfSloResult>,
  },

  library: {
    getInfo: () =>
      invokeTyped('get_library_info') as Promise<LibraryInfo>,
    close: () =>
      invoke<void>('close_library'),
    wipeImageData: () =>
      invokeTyped('wipe_image_data') as unknown as Promise<void>,
  },

  runtime: {
    getSnapshot: () =>
      invoke<RuntimeSnapshot>('get_runtime_snapshot'),
  },

  os: {
    openExternalUrl: (url: string) =>
      invokeTyped('open_external_url', { url }) as unknown as Promise<void>,
    openSettingsWindow: () =>
      invoke<void>('open_settings_window'),
    openSubscriptionsWindow: () =>
      invoke<void>('open_subscriptions_window'),
  },

  collections: {
    list: () =>
      invokeTyped('get_collections') as Promise<CollectionInfo[]>,
    getSummary: (id: number) =>
      invokeTyped('get_collection_summary', { id }) as Promise<CollectionSummary>,
    reorderMembers: (id: number, hashes: string[]) =>
      invokeTyped('reorder_collection_members', { id, hashes }) as unknown as Promise<void>,
    create: (params: { name: string }) =>
      invokeTyped('create_collection', params as never),
    addMembers: (params: { id: number; hashes: string[] }) =>
      invokeTyped('add_collection_members', params),
    removeMembers: (params: { id: number; hashes: string[] }) =>
      invokeTyped('remove_collection_members', params),
    update: (params: { id: number; name?: string; tags?: string[] }) =>
      invokeTyped('update_collection', {
        id: params.id,
        name: params.name,
        tags: params.tags,
      } as never) as unknown as Promise<void>,
    addTags: (id: number, tags: string[]) =>
      invokeTyped('add_collection_tags', { id, tags } as never) as unknown as Promise<void>,
    removeTags: (id: number, tags: string[]) =>
      invokeTyped('remove_collection_tags', { id, tags } as never) as unknown as Promise<void>,
    delete: (id: number) =>
      invokeTyped('delete_collection', { id }) as unknown as Promise<void>,
    listMemberHashes: (id: number) =>
      invokeTyped('list_collection_member_hashes', { id }) as unknown as Promise<string[]>,
  },

  companion: {
    getNamespaceValues: (namespace: string) =>
      invokeTyped('companion_get_namespace_values', { namespace }) as Promise<CompanionNamespaceValue[]>,
    getFilesByTag: (tag: string) =>
      invokeTyped('companion_get_files_by_tag', { tag }) as Promise<EntitySlim[]>,
  },
};

export const queryApi = {
  grid: {
    getPageSlim: api.grid.getPageSlim,
    getOutline: api.grid.getOutline,
    getEntitiesMetadataBatch: api.grid.getEntitiesMetadataBatch,
  },
  entity: {
    get: api.entity.get,
    getAllMetadata: api.entity.getAllMetadata,
    resolvePath: api.entity.resolvePath,
    resolveThumbnailPath: api.entity.resolveThumbnailPath,
  },
  files: {
    get: api.files.get,
    getAllMetadata: api.files.getAllMetadata,
    resolvePath: api.files.resolvePath,
    resolveThumbnailPath: api.files.resolveThumbnailPath,
  },
  file: {
    get: api.file.get,
    getAllMetadata: api.file.getAllMetadata,
    resolvePath: api.file.resolvePath,
    resolveThumbnailPath: api.file.resolveThumbnailPath,
  },
  selection: {
    getSummary: api.selection.getSummary,
    resolveHashes: api.selection.resolveHashes,
  },
  duplicates: {
    findSimilar: api.duplicates.findSimilar,
  },
  runtime: {
    getSnapshot: api.runtime.getSnapshot,
  },
};

export const commandApi = {
  entity: {
    setStatus: api.entity.setStatus,
    setStatusSelection: api.entity.setStatusSelection,
    deleteMany: api.entity.deleteMany,
    deleteSelection: api.entity.deleteSelection,
    updateRating: api.entity.updateRating,
    setName: api.entity.setName,
    setSourceUrls: api.entity.setSourceUrls,
    setNotes: api.entity.setNotes,
    openDefault: api.entity.openDefault,
    revealInFolder: api.entity.revealInFolder,
    openInNewWindow: api.entity.openInNewWindow,
    ensureThumbnail: api.entity.ensureThumbnail,
    regenerateThumbnail: api.entity.regenerateThumbnail,
    reanalyzeColors: api.entity.reanalyzeColors,
    regenerateThumbnailsBatch: api.entity.regenerateThumbnailsBatch,
  },
  files: {
    setStatus: api.files.setStatus,
    setStatusSelection: api.files.setStatusSelection,
    deleteMany: api.files.deleteMany,
    deleteSelection: api.files.deleteSelection,
    updateRating: api.files.updateRating,
    setName: api.files.setName,
    setSourceUrls: api.files.setSourceUrls,
    setNotes: api.files.setNotes,
    openDefault: api.files.openDefault,
    revealInFolder: api.files.revealInFolder,
    openInNewWindow: api.files.openInNewWindow,
    ensureThumbnail: api.files.ensureThumbnail,
    regenerateThumbnail: api.files.regenerateThumbnail,
    reanalyzeColors: api.files.reanalyzeColors,
    regenerateThumbnailsBatch: api.files.regenerateThumbnailsBatch,
  },
  file: {
    setStatus: api.file.setStatus,
    setStatusSelection: api.file.setStatusSelection,
    deleteMany: api.file.deleteMany,
    deleteSelection: api.file.deleteSelection,
    updateRating: api.file.updateRating,
    setName: api.file.setName,
    setSourceUrls: api.file.setSourceUrls,
    setNotes: api.file.setNotes,
    openDefault: api.file.openDefault,
    revealInFolder: api.file.revealInFolder,
    openInNewWindow: api.file.openInNewWindow,
    ensureThumbnail: api.file.ensureThumbnail,
    regenerateThumbnail: api.file.regenerateThumbnail,
    reanalyzeColors: api.file.reanalyzeColors,
    regenerateThumbnailsBatch: api.file.regenerateThumbnailsBatch,
  },
  selection: {
    addTags: api.selection.addTags,
    removeTags: api.selection.removeTags,
    updateRating: api.selection.updateRating,
    setNotes: api.selection.setNotes,
    setSourceUrls: api.selection.setSourceUrls,
  },
};
