export type UnlistenFn = () => void;

export class PhysicalSize {
  width: number;
  height: number;

  constructor(width: number, height: number) {
    this.width = width;
    this.height = height;
  }
}

function requireDesktop() {
  if (!window.picto?.api?.invoke) {
    throw new Error('Electron desktop API is unavailable. Start via Electron runtime.');
  }
  return window.picto;
}

export function invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T> {
  return requireDesktop().api.invoke<T>(command, args ?? {});
}

export function listen<T = unknown>(name: string, handler: (event: { payload: T }) => void): Promise<UnlistenFn> {
  return requireDesktop().events.on<T>(name, (payload) => handler({ payload }));
}

export async function emit<T = unknown>(name: string, payload: T): Promise<void> {
  await requireDesktop().events.emit(name, payload);
}

export async function emitTo<T = unknown>(target: string, name: string, payload: T): Promise<void> {
  await requireDesktop().events.emitTo(target, name, payload);
}

class DesktopWindow {
  async show(): Promise<void> {
    await requireDesktop().api.window?.call?.('show');
  }

  async setTheme(theme: string): Promise<void> {
    await requireDesktop().api.window?.call?.('setTheme', { theme });
  }

  async startDragging(): Promise<void> {
    await requireDesktop().api.window?.call?.('startDragging');
  }

  async minimize(): Promise<void> {
    await requireDesktop().api.window?.call?.('minimize');
  }

  async toggleMaximize(): Promise<void> {
    await requireDesktop().api.window?.call?.('toggleMaximize');
  }

  async setSize(size: PhysicalSize): Promise<void> {
    await requireDesktop().api.window?.call?.('setSize', { width: size.width, height: size.height });
  }

  async setAlwaysOnTop(value: boolean): Promise<void> {
    await requireDesktop().api.window?.call?.('setAlwaysOnTop', { value });
  }

  async close(): Promise<void> {
    await requireDesktop().api.window?.call?.('close');
  }

  async setFocus(): Promise<void> {
    await requireDesktop().api.window?.call?.('setFocus');
  }

  async isMaximized(): Promise<boolean> {
    return Boolean(await requireDesktop().api.window?.call?.('isMaximized'));
  }

  async outerPosition(): Promise<{ x: number; y: number }> {
    const value = await requireDesktop().api.window?.call?.('outerPosition');
    return (value as { x: number; y: number }) ?? { x: 0, y: 0 };
  }

  async innerSize(): Promise<{ width: number; height: number }> {
    const value = await requireDesktop().api.window?.call?.('innerSize');
    return (value as { width: number; height: number }) ?? { width: window.innerWidth, height: window.innerHeight };
  }

  async onResized(handler: (event: { payload: { width: number; height: number } }) => void): Promise<UnlistenFn> {
    return requireDesktop().events.on('picto:window-resized', (payload) => {
      handler({ payload: payload as { width: number; height: number } });
    });
  }

  async onMoved(handler: () => void): Promise<UnlistenFn> {
    return requireDesktop().events.on('picto:window-moved', () => handler());
  }
}

export function getCurrentWindow(): DesktopWindow {
  return new DesktopWindow();
}

export async function currentMonitor(): Promise<{ scaleFactor: number; size: { width: number; height: number } } | null> {
  if (!window.picto?.monitor?.current) return null;
  return window.picto.monitor.current();
}

export async function setTheme(theme: string): Promise<void> {
  await requireDesktop().api.window?.call?.('setTheme', { theme });
}

export function popupMenu(): Promise<void> {
  return requireDesktop().api.popupMenu?.() ?? Promise.resolve();
}

export function getCurrentWebview() {
  return {
    onDragDropEvent: (handler: (event: { payload: unknown }) => void) => {
      if (!window.picto?.webview?.onDragDropEvent) return Promise.resolve(() => {});
      return window.picto.webview.onDragDropEvent(handler as any);
    },
    startNativeDrag: (hashes: string[], iconDataUrl?: string | null) => {
      if (!window.picto?.webview?.startNativeDrag) return Promise.resolve(null);
      return window.picto.webview.startNativeDrag(hashes, iconDataUrl);
    },
  };
}

export async function open(options?: Record<string, unknown>): Promise<string | string[] | null> {
  return window.picto?.dialog?.open ? window.picto.dialog.open(options) : null;
}

export async function save(options?: Record<string, unknown>): Promise<string | null> {
  return window.picto?.dialog?.save ? window.picto.dialog.save(options) : null;
}

export async function writeText(text: string): Promise<void> {
  if (window.picto?.clipboard?.writeText) {
    await window.picto.clipboard.writeText(text);
    return;
  }
  await navigator.clipboard.writeText(text);
}

export async function copyFileToClipboard(filePath: string): Promise<void> {
  if (window.picto?.clipboard?.copyFile) {
    await window.picto.clipboard.copyFile(filePath);
    return;
  }
  // Fallback: copy path as text
  await navigator.clipboard.writeText(filePath);
}

export async function copyImageToClipboard(filePath: string): Promise<void> {
  if (window.picto?.clipboard?.copyImage) {
    await window.picto.clipboard.copyImage(filePath);
    return;
  }
  throw new Error('Image clipboard not available outside Electron');
}

export type ReverseImageEngine = 'tineye' | 'saucenao' | 'yandex' | 'sogou' | 'bing';

export async function reverseImageSearch(filePath: string, engine: ReverseImageEngine): Promise<string> {
  if (window.picto?.search?.reverseImage) {
    return window.picto.search.reverseImage(filePath, engine);
  }
  throw new Error('Reverse image search not available outside Electron');
}

export interface LibraryConfig {
  currentPath: string | null;
  libraryHistory: string[];
  pinnedLibraries: string[];
  existsMap: Record<string, boolean>;
}

export const libraryHost = {
  getConfig: async (): Promise<LibraryConfig> =>
    await window.picto?.library?.getConfig?.() ?? { currentPath: null, libraryHistory: [], pinnedLibraries: [], existsMap: {} },
  create: async (name: string, savePath: string): Promise<void> => {
    await window.picto?.library?.create?.(name, savePath);
  },
  open: async (): Promise<void> => {
    await window.picto?.library?.open?.();
  },
  switch: async (path: string): Promise<void> => {
    await window.picto?.library?.switch?.(path);
  },
  remove: async (path: string): Promise<void> => {
    await window.picto?.library?.remove?.(path);
  },
  delete: async (path: string): Promise<void> => {
    await window.picto?.library?.delete?.(path);
  },
  togglePin: async (path: string): Promise<void> => {
    await window.picto?.library?.togglePin?.(path);
  },
  rename: async (path: string, newName: string): Promise<void> => {
    await window.picto?.library?.rename?.(path, newName);
  },
  relocate: async (oldPath: string): Promise<void> => {
    await window.picto?.library?.relocate?.(oldPath);
  },
};

export interface Store {
  get<T = unknown>(key: string): Promise<T | null>;
  set(key: string, value: unknown): Promise<void>;
  save(): Promise<void>;
  onKeyChange(key: string, handler: (value: unknown) => void): Promise<UnlistenFn>;
}

const channels = new Map<string, BroadcastChannel>();

class LocalStore implements Store {
  private namespace: string;
  private state: Record<string, unknown>;

  constructor(name: string) {
    this.namespace = `picto:store:${name}`;
    const raw = localStorage.getItem(this.namespace);
    // PBI-036: Harden deserialization — quarantine corrupt data, reset to empty.
    if (raw) {
      try {
        const parsed = JSON.parse(raw);
        this.state = (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) ? parsed : {};
      } catch {
        console.warn(`[LocalStore] corrupt JSON in "${this.namespace}"; quarantining and resetting`);
        try { localStorage.setItem(`${this.namespace}:quarantine`, raw); } catch { /* best effort */ }
        this.state = {};
      }
    } else {
      this.state = {};
    }
  }

  async get<T = unknown>(key: string): Promise<T | null> {
    return (this.state[key] as T | undefined) ?? null;
  }

  async set(key: string, value: unknown): Promise<void> {
    this.state[key] = value;
  }

  async save(): Promise<void> {
    localStorage.setItem(this.namespace, JSON.stringify(this.state));
    getChannel(this.namespace).postMessage({ type: 'save', payload: this.state });
  }

  async onKeyChange(key: string, handler: (value: unknown) => void): Promise<UnlistenFn> {
    const channel = getChannel(this.namespace);
    const listener = (event: MessageEvent) => {
      if (event.data?.type !== 'save') return;
      const next = event.data.payload ?? {};
      if (Object.prototype.hasOwnProperty.call(next, key)) handler(next[key]);
    };
    channel.addEventListener('message', listener);
    return () => channel.removeEventListener('message', listener);
  }
}

function getChannel(name: string): BroadcastChannel {
  let channel = channels.get(name);
  if (!channel) {
    channel = new BroadcastChannel(name);
    channels.set(name, channel);
  }
  return channel;
}

export async function load(name: string, _options?: { autoSave?: boolean }): Promise<Store> {
  return new LocalStore(name);
}

import type {
  ImageItem,
  GridPageSlimResponse, GridPageSlimQuery,
  FileAllMetadata, FileMetadataBatchResponse, EnsureThumbnailResponse,
  ImportResult, BackfillBlurhashResult,
  TagDisplay, TagSearchResult, TagTuple, TagRecord,
  NamespaceSummary, TagAlias, TagRelation,
  RenameTagResult, DeleteTagResult, NormalizeNamespacesResult,
  SelectionQuerySpec, SelectionSummary,
  Folder, FolderMembership, FolderReorderMove,
  SmartFolder, SmartFolderPredicate,
  SidebarTreeResponse,
  DuplicateInfo,
  ScanDuplicatesResult, DuplicatePairsResponse, DuplicateSettings,
  SmartMergeResult, ResolveDuplicateAction,
  SubscriptionInfo, SubscriptionQueryInfo, FlowInfo,
  SubscriptionProgressEvent,
  SubscriptionSiteInfo, SiteMetadataSchema, SiteMetadataValidationResult,
  CredentialDomain, CredentialType, CredentialHealth,
  PtrStats, PtrSyncPerfBreakdown,
  PtrSyncProgress, PtrBootstrapStatus, PtrCompactIndexStatus,
  AppSettings, StorageStats,
  CollectionInfo, CollectionSummary, ReviewQueueItem, CompanionNamespaceValue,
  ViewPrefsDto, ViewPrefsPatch,
  FileStats, PerfSnapshot, PerfSloResult,
  ColorSearchResult, LibraryInfo,
} from '../types/api';

export { api as desktopTypedApi };

/** Normalize backend SmartFolder shape (smart_folder_id → id, predicate_json → predicate). */
function normalizeSmartFolder(r: Record<string, unknown>): SmartFolder {
  return {
    id: String(r.smart_folder_id ?? r.id ?? ''),
    name: String(r.name ?? ''),
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

/**
 * Typed API surface — single place where all backend command strings live.
 * Every invoke() in the codebase should route through here.
 */
export const api = {
  grid: {
    getPageSlim: (query: GridPageSlimQuery) =>
      invoke<GridPageSlimResponse>('get_grid_page_slim', { query }),
    getFilesMetadataBatch: (hashes: string[]) =>
      invoke<FileMetadataBatchResponse>('get_files_metadata_batch', { hashes }),
    getFileCount: () =>
      invoke<number>('get_file_count'),
  },

  file: {
    get: (hash: string) =>
      invoke<ImageItem | null>('get_file', { hash }),
    getAllMetadata: (hash: string) =>
      invoke<FileAllMetadata>('get_file_all_metadata', { hash }),
    setStatus: (hash: string, status: string) =>
      invoke<void>('update_file_status', { hash, status }),
    setStatusSelection: (selection: SelectionQuerySpec, status: string) =>
      invoke<number>('update_file_status_selection', { selection, status }),
    delete: (hash: string) =>
      invoke<void>('delete_file', { hash }),
    deleteMany: (hashes: string[]) =>
      invoke<number>('delete_files', { hashes }),
    deleteSelection: (selection: SelectionQuerySpec) =>
      invoke<number>('delete_files_selection', { selection }),
    updateRating: (hash: string, rating: number | null) =>
      invoke<void>('update_rating', { hash, rating }),
    setName: (hash: string, name: string | null) =>
      invoke<void>('set_file_name', { hash, name }),
    setSourceUrls: (hash: string, urls: string[]) =>
      invoke<void>('set_source_urls', { hash, urls }),
    getNotes: (hash: string) =>
      invoke<Record<string, string>>('get_file_notes', { hash }),
    setNotes: (hash: string, notes: Record<string, string>) =>
      invoke<void>('set_file_notes', { hash, notes }),
    incrementViewCount: (hash: string) =>
      invoke<void>('increment_view_count', { hash }),
    resolvePath: (hash: string) =>
      invoke<string>('resolve_file_path', { hash }),
    resolveThumbnailPath: (hash: string) =>
      invoke<string>('resolve_thumbnail_path', { hash }),
    openDefault: (hash: string) =>
      invoke<void>('open_file_default', { hash }),
    revealInFolder: (hash: string) =>
      invoke<void>('reveal_in_folder', { hash }),
    openInNewWindow: (hash: string, width?: number | null, height?: number | null) =>
      invoke<void>('open_in_new_window', { hash, width: width ?? null, height: height ?? null }),
    export: (hash: string, destPath: string) =>
      invoke<void>('export_file', { hash, dest_path: destPath }),
    ensureThumbnail: (hash: string) =>
      invoke<EnsureThumbnailResponse>('ensure_thumbnail', { hash }),
    regenerateThumbnail: (hash: string) =>
      invoke<EnsureThumbnailResponse>('regenerate_thumbnail', { hash }),
    regenerateThumbnailsBatch: (hashes: string[]) =>
      invoke<{ total: number; regenerated: number; errors: number }>('regenerate_thumbnails_batch', { hashes }),
    getParents: (hash: string) =>
      invoke<string[]>('get_file_parents', { hash }),
    getThumbnailBytes: (imageId: number) =>
      invoke<number[]>('get_image_thumbnail', { imageId }),
  },

  import: {
    files: (paths: string[], tagStrings?: string[], sourceUrls?: string[], initialStatus?: number) =>
      invoke<ImportResult>('import_files', { paths, tag_strings: tagStrings, source_urls: sourceUrls, initial_status: initialStatus }),
    rebuildFts: () =>
      invoke<void>('rebuild_file_fts'),
    backfillBlurhashes: (limit?: number) =>
      invoke<BackfillBlurhashResult>('backfill_missing_blurhashes', { limit }),
  },

  tags: {
    search: (query: string, limit?: number) =>
      invoke<TagSearchResult[]>('search_tags', { query, limit }),
    getAll: () =>
      invoke<TagTuple[]>('get_all_tags_with_counts'),
    getForFile: (hash: string) =>
      invoke<TagDisplay[]>('get_file_tags', { hash }),
    getForFileDisplay: (hash: string) =>
      invoke<TagDisplay[]>('get_file_tags_display', { hash }),
    add: (hash: string, tagStrings: string[]) =>
      invoke<void>('add_tags', { hash, tag_strings: tagStrings }),
    remove: (hash: string, tagStrings: string[]) =>
      invoke<void>('remove_tags', { hash, tag_strings: tagStrings }),
    addBatch: (hashes: string[], tagStrings: string[]) =>
      invoke<void>('add_tags_batch', { hashes, tag_strings: tagStrings }),
    removeBatch: (hashes: string[], tagStrings: string[]) =>
      invoke<void>('remove_tags_batch', { hashes, tag_strings: tagStrings }),
    findFilesByTags: (tagStrings: string[], limit?: number, offset?: number) =>
      invoke<string[]>('find_files_by_tags', { tag_strings: tagStrings, limit, offset }),
    getPaginated: (params: { namespace?: string; search?: string; cursor?: string; limit?: number }) =>
      invoke<TagRecord[]>('get_tags_paginated', params),
    getNamespaceSummary: () =>
      invoke<NamespaceSummary[]>('get_namespace_summary'),
    lookupTypes: () =>
      invoke<string[]>('lookup_tag_types'),
    getAliases: () =>
      invoke<TagAlias[]>('get_tag_aliases'),
    setAlias: (from: string, to: string) =>
      invoke<void>('set_tag_alias', { from, to }),
    removeAlias: (from: string) =>
      invoke<void>('remove_tag_alias', { from }),
    getSiblings: (tagId: number) =>
      invoke<TagRelation[]>('get_tag_siblings_for_tag', { tag_id: tagId }),
    getParents: (tagId: number) =>
      invoke<TagRelation[]>('get_tag_parents_for_tag', { tag_id: tagId }),
    addParent: (child: string, parent: string) =>
      invoke<void>('add_tag_parent', { child, parent }),
    removeParent: (child: string, parent: string) =>
      invoke<void>('remove_tag_parent', { child, parent }),
    merge: (fromTag: string, toTag: string) =>
      invoke<void>('merge_tags', { from_tag: fromTag, to_tag: toTag }),
    rename: (tagId: number, newName: string) =>
      invoke<RenameTagResult>('rename_tag', { tag_id: tagId, new_name: newName }),
    delete: (tagId: number) =>
      invoke<DeleteTagResult>('delete_tag', { tag_id: tagId }),
    normalizeNamespaces: () =>
      invoke<NormalizeNamespacesResult>('normalize_ingested_namespaces'),
    searchPaged: (query: string, limit: number, offset: number) =>
      invoke<[string, string, number][]>('search_tags_paged', { query, limit, offset }),
  },

  selection: {
    getSummary: (selection: SelectionQuerySpec) =>
      invoke<SelectionSummary>('get_selection_summary', { selection }),
    addTags: (selection: SelectionQuerySpec, tagStrings: string[]) =>
      invoke<number>('add_tags_selection', { selection, tagStrings }),
    removeTags: (selection: SelectionQuerySpec, tagStrings: string[]) =>
      invoke<number>('remove_tags_selection', { selection, tagStrings }),
    updateRating: (selection: SelectionQuerySpec, rating: number | null) =>
      invoke<number>('update_rating_selection', { selection, rating }),
    setNotes: (selection: SelectionQuerySpec, notes: Record<string, string>) =>
      invoke<number>('set_notes_selection', { selection, notes }),
    setSourceUrls: (selection: SelectionQuerySpec, urls: string[]) =>
      invoke<number>('set_source_urls_selection', { selection, urls }),
  },

  folders: {
    list: () =>
      invoke<Folder[]>('list_folders'),
    create: (params: { name: string; parent_id?: number | null; icon?: string; color?: string }) =>
      invoke<Folder>('create_folder', params),
    update: (params: { folder_id: number; name?: string; icon?: string; color?: string }) =>
      invoke<void>('update_folder', params),
    delete: (folderId: number) =>
      invoke<void>('delete_folder', { folder_id: folderId }),
    updateParent: (folderId: number, newParentId?: number | null) =>
      invoke<void>('update_folder_parent', { folder_id: folderId, new_parent_id: newParentId }),
    // PBI-057: Atomic move_folder — reparent + reorder in one transaction.
    moveFolder: (folderId: number, newParentId: number | null, siblingOrder: [number, number][]) =>
      invoke<void>('move_folder', { folder_id: folderId, new_parent_id: newParentId, sibling_order: siblingOrder }),
    addFile: (folderId: number, hash: string) =>
      invoke<void>('add_file_to_folder', { folder_id: folderId, hash }),
    // PBI-054: Batch add files to folder.
    addFilesBatch: (folderId: number, hashes: string[]) =>
      invoke<number>('add_files_to_folder_batch', { folder_id: folderId, hashes }),
    removeFile: (folderId: number, hash: string) =>
      invoke<void>('remove_file_from_folder', { folder_id: folderId, hash }),
    removeFilesBatch: (folderId: number, hashes: string[]) =>
      invoke<number>('remove_files_from_folder_batch', { folder_id: folderId, hashes }),
    getFiles: (folderId: number) =>
      invoke<string[]>('get_folder_files', { folder_id: folderId }),
    getCoverHash: (folderId: number) =>
      invoke<string | null>('get_folder_cover_hash', { folder_id: folderId }),
    getFileFolders: (hash: string) =>
      invoke<FolderMembership[]>('get_file_folders', { hash }),
    getEntityFolders: (entityId: number) =>
      invoke<FolderMembership[]>('get_entity_folders', { entity_id: entityId }),
    reorder: (moves: [number, number][]) =>
      invoke<void>('reorder_folders', { moves }),
    reorderItems: (folderId: number, moves: FolderReorderMove[]) =>
      invoke<void>('reorder_folder_items', { folder_id: folderId, moves }),
    sortItems: (folderId: number, sortBy: string, direction: string, hashes?: string[]) =>
      invoke<void>('sort_folder_items', { folder_id: folderId, sort_by: sortBy, direction, hashes }),
    reverseItems: (folderId: number, hashes?: string[]) =>
      invoke<void>('reverse_folder_items', { folder_id: folderId, hashes }),
  },

  smartFolders: {
    list: async (): Promise<SmartFolder[]> => {
      const raw = await invoke<Array<Record<string, unknown>>>('list_smart_folders');
      return raw.map(normalizeSmartFolder);
    },
    create: async (folder: SmartFolder): Promise<SmartFolder> => {
      const raw = await invoke<Record<string, unknown>>('create_smart_folder', { folder });
      return normalizeSmartFolder(raw);
    },
    update: async (id: string, folder: SmartFolder): Promise<SmartFolder> => {
      const raw = await invoke<Record<string, unknown>>('update_smart_folder', { id, folder });
      return normalizeSmartFolder(raw);
    },
    delete: (id: string) =>
      invoke<void>('delete_smart_folder', { id }),
    query: (predicate: SmartFolderPredicate, limit?: number, offset?: number) =>
      invoke<string[]>('query_smart_folder', { predicate, limit, offset }),
    count: (predicate: SmartFolderPredicate) =>
      invoke<number>('count_smart_folder', { predicate }),
    reorder: (moves: [number, number][]) =>
      invoke<void>('reorder_smart_folders', { moves }),
  },

  sidebar: {
    getTree: () =>
      invoke<SidebarTreeResponse>('get_sidebar_tree'),
    reorderNodes: (moves: [string, number][]) =>
      invoke<void>('reorder_sidebar_nodes', { moves }),
  },

  duplicates: {
    getForFile: (hash: string) =>
      invoke<DuplicateInfo[]>('get_duplicates', { hash }),
    getPairs: (cursor?: string | null, limit?: number, status?: string) =>
      invoke<DuplicatePairsResponse>('get_duplicate_pairs', {
        ...(cursor ? { cursor } : {}),
        ...(limit ? { limit } : {}),
        ...(status ? { status } : {}),
      }),
    resolvePair: (action: ResolveDuplicateAction, hashA: string, hashB: string) =>
      invoke<SmartMergeResult | Record<string, string>>('resolve_duplicate_pair', {
        action,
        hash_a: hashA,
        hash_b: hashB,
      }),
    getCount: () =>
      invoke<{ count: number }>('get_duplicate_count'),
    scan: () =>
      invoke<ScanDuplicatesResult>('scan_duplicates'),
    getSettings: () =>
      invoke<DuplicateSettings>('get_duplicate_settings'),
    updateSettings: (settings: Partial<DuplicateSettings>) =>
      invoke<{ ok: boolean }>('update_duplicate_settings', settings),
  },

  subscriptions: {
    list: () =>
      invoke<SubscriptionInfo[]>('get_subscriptions'),
    create: (params: {
      name: string;
      site_id: string;
      queries: string[];
      flow_id?: number;
      initial_file_limit?: number;
      periodic_file_limit?: number;
    }) =>
      invoke<SubscriptionInfo>('create_subscription', params),
    delete: (id: string, deleteFiles?: boolean) =>
      invoke<number>('delete_subscription', { id, delete_files: deleteFiles }),
    rename: (id: string, name: string) =>
      invoke<void>('rename_subscription', { id, name }),
    pause: (id: string, paused: boolean) =>
      invoke<void>('pause_subscription', { id, paused }),
    run: (id: string) =>
      invoke<void>('run_subscription', { id }),
    stop: (id: string) =>
      invoke<void>('stop_subscription', { id }),
    reset: (id: string) =>
      invoke<void>('reset_subscription', { id }),
    getRunning: () =>
      invoke<string[]>('get_running_subscriptions'),
    getRunningProgress: () =>
      invoke<SubscriptionProgressEvent[]>('get_running_subscription_progress'),
    addQuery: (subscriptionId: string, queryText: string) =>
      invoke<SubscriptionQueryInfo>('add_subscription_query', { subscription_id: subscriptionId, query_text: queryText }),
    deleteQuery: (id: string) =>
      invoke<void>('delete_subscription_query', { id }),
    pauseQuery: (id: string, paused: boolean) =>
      invoke<void>('pause_subscription_query', { id, paused }),
    runQuery: (subscriptionId: string, queryId: string) =>
      invoke<void>('run_subscription_query', { subscription_id: subscriptionId, query_id: queryId }),
    getSites: () =>
      invoke<SubscriptionSiteInfo[]>('get_sites'),
    getSiteMetadataSchema: (siteId: string) =>
      invoke<SiteMetadataSchema>('get_site_metadata_schema', { site_id: siteId }),
    validateSiteMetadata: (params: {
      site_id: string;
      sample_url?: string;
      sample_metadata_json?: Record<string, unknown> | null;
    }) =>
      invoke<SiteMetadataValidationResult>('validate_site_metadata', params),
    listCredentials: () =>
      invoke<CredentialDomain[]>('list_credentials'),
    listCredentialHealth: () =>
      invoke<CredentialHealth[]>('list_credential_health'),
    setCredential: (params: {
      site_category: string;
      credential_type: CredentialType;
      display_name?: string | null;
      username?: string | null;
      password?: string | null;
      cookies?: Record<string, string> | null;
      oauth_token?: string | null;
    }) =>
      invoke<void>('set_credential', params),
    deleteCredential: (siteCategory: string) =>
      invoke<void>('delete_credential', { site_category: siteCategory }),
  },

  flows: {
    list: () =>
      invoke<FlowInfo[]>('get_flows'),
    create: (name: string, schedule?: string) =>
      invoke<FlowInfo>('create_flow', { name, schedule }),
    delete: (id: string, deleteFiles?: boolean) =>
      invoke<void>('delete_flow', { id, delete_files: deleteFiles }),
    rename: (id: string, name: string) =>
      invoke<void>('rename_flow', { id, name }),
    setSchedule: (id: string, schedule: string) =>
      invoke<void>('set_flow_schedule', { id, schedule }),
    run: (id: string) =>
      invoke<void>('run_flow', { id }),
    stop: (id: string) =>
      invoke<void>('stop_flow', { id }),
  },

  ptr: {
    getStatus: () =>
      invoke<PtrStats>('get_ptr_status'),
    isSyncing: () =>
      invoke<boolean>('is_ptr_syncing'),
    getSyncProgress: () =>
      invoke<PtrSyncProgress | null>('get_ptr_sync_progress'),
    sync: () =>
      invoke<{ id: string; message: string }>('ptr_sync'),
    cancelSync: () =>
      invoke<void>('cancel_ptr_sync'),
    cancelBootstrap: () =>
      invoke<void>('ptr_cancel_bootstrap'),
    bootstrapFromSnapshot: (req: { snapshot_dir: string; ptr_service_id?: number | null; mode: string }) =>
      invoke<Record<string, unknown>>('ptr_bootstrap_from_hydrus_snapshot', req),
    getBootstrapStatus: () =>
      invoke<PtrBootstrapStatus>('ptr_get_bootstrap_status'),
    getCompactIndexStatus: () =>
      invoke<PtrCompactIndexStatus>('ptr_get_compact_index_status'),
    getNamespaceSummary: () =>
      invoke<NamespaceSummary[]>('ptr_get_namespace_summary'),
    getTagsPaginated: (params: { namespace?: string; search?: string; cursor?: string; limit?: number }) =>
      invoke<TagRecord[]>('ptr_get_tags_paginated', params),
    getTagSiblings: (tagId: number) =>
      invoke<TagRelation[]>('ptr_get_tag_siblings', { tag_id: tagId }),
    getTagParents: (tagId: number) =>
      invoke<TagRelation[]>('ptr_get_tag_parents', { tag_id: tagId }),
    getSyncPerfBreakdown: () =>
      invoke<PtrSyncPerfBreakdown>('get_ptr_sync_perf_breakdown'),
  },

  settings: {
    get: () =>
      invoke<AppSettings>('get_settings'),
    save: (settings: Partial<AppSettings>) =>
      invoke<void>('save_settings', settings as Record<string, unknown>),
    getViewPrefs: (scopeKey?: string) =>
      invoke<ViewPrefsDto | null>('get_view_prefs', { scope_key: scopeKey }),
    setViewPrefs: (scopeKey: string | undefined, patch: ViewPrefsPatch) =>
      invoke<ViewPrefsDto>('set_view_prefs', { scope_key: scopeKey, patch }),
    setZoomFactor: (factor: number) =>
      invoke<void>('set_zoom_factor', { factor }),
    getZoomFactor: () =>
      invoke<number>('get_zoom_factor'),
  },

  stats: {
    getStorageStats: () =>
      invoke<StorageStats>('get_storage_stats'),
    getImageStorageStats: () =>
      invoke<FileStats>('get_image_storage_stats'),
    getPerfSnapshot: () =>
      invoke<PerfSnapshot>('get_perf_snapshot'),
    checkPerfSlo: () =>
      invoke<PerfSloResult>('check_perf_slo'),
  },

  library: {
    getInfo: () =>
      invoke<LibraryInfo>('get_library_info'),
    close: () =>
      invoke<void>('close_library'),
    wipeImageData: () =>
      invoke<void>('wipe_image_data'),
  },

  color: {
    searchByColor: (hexColor: string, maxDistance?: number) =>
      invoke<ColorSearchResult[]>('search_by_color', { hex_color: hexColor, max_distance: maxDistance }),
  },

  os: {
    openExternalUrl: (url: string) =>
      invoke<void>('open_external_url', { url }),
    enableModernWindowStyle: (cornerRadius: number) =>
      invoke<void>('enable_modern_window_style', { cornerRadius }),
    openSettingsWindow: () =>
      invoke<void>('open_settings_window'),
    openSubscriptionsWindow: () =>
      invoke<void>('open_subscriptions_window'),
  },

  collections: {
    list: () =>
      invoke<CollectionInfo[]>('get_collections'),
    getSummary: (id: number) =>
      invoke<CollectionSummary>('get_collection_summary', { id }),
    setRating: (id: number, rating: number | null) =>
      invoke<void>('set_collection_rating', { id, rating }),
    setSourceUrls: (id: number, sourceUrls: string[]) =>
      invoke<void>('set_collection_source_urls', { id, source_urls: sourceUrls }),
    reorderMembers: (id: number, hashes: string[]) =>
      invoke<void>('reorder_collection_members', { id, hashes }),
    create: (params: { name: string; description?: string | null; tags?: string[] }) =>
      invoke<number>('create_collection', params),
    addMembers: (params: { id: number; hashes: string[] }) =>
      invoke<number>('add_collection_members', params),
    removeMembers: (params: { id: number; hashes: string[] }) =>
      invoke<number>('remove_collection_members', params),
    update: (params: { id: number; name?: string; description?: string | null; tags?: string[]; sourceUrls?: string[] }) =>
      invoke<void>('update_collection', {
        id: params.id,
        name: params.name,
        description: params.description,
        tags: params.tags,
        source_urls: params.sourceUrls,
      }),
    delete: (id: number) =>
      invoke<void>('delete_collection', { id }),
  },

  review: {
    getQueue: () =>
      invoke<ReviewQueueItem[]>('get_review_queue'),
    getItemImage: (hash: string) =>
      invoke<number[]>('get_review_item_image', { hash }),
    action: (hash: string, action: string) =>
      invoke<void>('review_image_action', { hash, action: { action } }),
  },

  companion: {
    getNamespaceValues: (namespace: string) =>
      invoke<CompanionNamespaceValue[]>('companion_get_namespace_values', { namespace }),
    getFilesByTag: (tag: string) =>
      invoke<ImageItem[]>('companion_get_files_by_tag', { tag }),
  },
};
