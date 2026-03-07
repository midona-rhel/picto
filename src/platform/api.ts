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

export async function setTheme(theme: string): Promise<void> {
  await requireDesktop().api.window?.call?.('setTheme', { theme });
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

import type { RuntimeSnapshot } from '../shared/types/generated/runtime-contract';
import type { CoreRuntimeEventPayloadMap } from '../shared/types/api/events';

import type {
  EntityAllMetadata,
  EntityDetails,
  EntityMetadataBatchResponse,
  EntitySlim,
  GridPageSlimResponse, GridPageSlimQuery,
  EnsureThumbnailResponse, ReanalyzeFileColorsResponse,
  ImportResult,
  TagDisplay, TagSearchResult, TagTuple, TagRecord,
  NamespaceSummary, TagRelation,
  RenameTagResult, DeleteTagResult, NormalizeNamespacesResult,
  SelectionQuerySpec, SelectionSummary,
  Folder, FolderMembership, FolderReorderMove,
  SmartFolder, SmartFolderPredicate,
  SidebarTreeResponse,
  ScanDuplicatesResult, DuplicatePairsResponse, DuplicateSettings,
  SmartMergeResult, ResolveDuplicateAction,
  SubscriptionInfo, SubscriptionQueryInfo, FlowInfo,
  SubscriptionProgressEvent,
  SubscriptionSiteInfo, SiteMetadataSchema, SiteMetadataValidationResult,
  CredentialDomain, CredentialType, CredentialHealth,
  PtrStats, PtrSyncPerfBreakdown,
  PtrSyncProgress, PtrBootstrapStatus, PtrCompactIndexStatus,
  AppSettings,
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

// ─── Typed command dispatch (PBI-234) ──────────────────────────────────────
//
// Generated types live in types/generated/commands/ (via ts-rs from Rust).
// invokeTyped() provides compile-time checked command names and argument types.

import type { TypedCommandMap } from '../shared/types/generated/commands';

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
      invokeTyped('get_grid_page_slim', { query } as never) as Promise<GridPageSlimResponse>,
    getFilesMetadataBatch: (hashes: string[]) =>
      invokeTyped('get_files_metadata_batch', { hashes }) as Promise<EntityMetadataBatchResponse>,
    getFileCount: () =>
      invokeTyped('get_file_count') as Promise<number>,
  },

  file: {
    get: (hash: string) =>
      invokeTyped('get_file', { hash }) as Promise<EntityDetails | null>,
    getAllMetadata: (hash: string) =>
      invokeTyped('get_file_all_metadata', { hash }) as Promise<EntityAllMetadata>,
    setStatus: (hash: string, status: string) =>
      invokeTyped('update_file_status', { hash, status }) as unknown as Promise<void>,
    setStatusSelection: (selection: SelectionQuerySpec, status: string) =>
      invokeTyped('update_file_status_selection', { selection, status } as never),
    deleteMany: (hashes: string[]) =>
      invokeTyped('delete_files', { hashes }),
    deleteSelection: (selection: SelectionQuerySpec) =>
      invokeTyped('delete_files_selection', { selection } as never),
    updateRating: (hash: string, rating: number | null) =>
      invokeTyped('update_rating', { hash, rating }) as unknown as Promise<void>,
    setName: (hash: string, name: string | null) =>
      invokeTyped('set_file_name', { hash, name }) as unknown as Promise<void>,
    setSourceUrls: (hash: string, urls: string[]) =>
      invokeTyped('set_source_urls', { hash, urls }) as unknown as Promise<void>,
    setNotes: (hash: string, notes: Record<string, string>) =>
      invokeTyped('set_file_notes', { hash, notes }) as unknown as Promise<void>,
    incrementViewCount: (hash: string) =>
      invokeTyped('increment_view_count', { hash }) as unknown as Promise<void>,
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
  },

  import: {
    files: (paths: string[], tagStrings?: string[], sourceUrls?: string[], initialStatus?: number) =>
      invokeTyped('import_files', { paths, tag_strings: tagStrings, source_urls: sourceUrls, initial_status: initialStatus } as never) as unknown as Promise<ImportResult>,
  },

  tags: {
    search: (query: string, limit?: number) =>
      invokeTyped('search_tags', { query, limit } as never) as Promise<TagSearchResult[]>,
    getAll: () =>
      invokeTyped('get_all_tags_with_counts') as Promise<TagTuple[]>,
    getForFile: (hash: string) =>
      invokeTyped('get_file_tags', { hash }) as Promise<TagDisplay[]>,
    add: (hash: string, tagStrings: string[]) =>
      invokeTyped('add_tags', { hash, tag_strings: tagStrings }) as unknown as Promise<void>,
    remove: (hash: string, tagStrings: string[]) =>
      invokeTyped('remove_tags', { hash, tag_strings: tagStrings }) as unknown as Promise<void>,
    addBatch: (hashes: string[], tagStrings: string[]) =>
      invokeTyped('add_tags_batch', { hashes, tag_strings: tagStrings }) as unknown as Promise<void>,
    removeBatch: (hashes: string[], tagStrings: string[]) =>
      invokeTyped('remove_tags_batch', { hashes, tag_strings: tagStrings }) as unknown as Promise<void>,
    findFilesByTags: (tagStrings: string[], limit?: number, offset?: number) =>
      invokeTyped('find_files_by_tags', { tag_strings: tagStrings, limit, offset } as never) as Promise<string[]>,
    getPaginated: (params: { namespace?: string; search?: string; cursor?: string; limit?: number }) =>
      invokeTyped('get_tags_paginated', params as never) as Promise<TagRecord[]>,
    getNamespaceSummary: () =>
      invokeTyped('get_namespace_summary') as Promise<NamespaceSummary[]>,
    setAlias: (from: string, to: string) =>
      invokeTyped('set_tag_alias', { from, to }) as unknown as Promise<void>,
    removeAlias: (from: string) =>
      invokeTyped('remove_tag_alias', { from }) as unknown as Promise<void>,
    getSiblings: (tagId: number) =>
      invokeTyped('get_tag_siblings_for_tag', { tag_id: tagId }) as Promise<TagRelation[]>,
    getParents: (tagId: number) =>
      invokeTyped('get_tag_parents_for_tag', { tag_id: tagId }) as Promise<TagRelation[]>,
    addParent: (child: string, parent: string) =>
      invokeTyped('add_tag_parent', { child, parent }) as unknown as Promise<void>,
    removeParent: (child: string, parent: string) =>
      invokeTyped('remove_tag_parent', { child, parent }) as unknown as Promise<void>,
    merge: (fromTag: string, toTag: string) =>
      invokeTyped('merge_tags', { from_tag: fromTag, to_tag: toTag }) as unknown as Promise<void>,
    rename: (tagId: number, newName: string) =>
      invokeTyped('rename_tag', { tag_id: tagId, new_name: newName }) as Promise<RenameTagResult>,
    delete: (tagId: number) =>
      invokeTyped('delete_tag', { tag_id: tagId }) as Promise<DeleteTagResult>,
    normalizeNamespaces: () =>
      invokeTyped('normalize_ingested_namespaces') as Promise<NormalizeNamespacesResult>,
    searchPaged: (query: string, limit: number, offset: number) =>
      invokeTyped('search_tags_paged', { query, limit, offset } as never) as Promise<[string, string, number][]>,
  },

  selection: {
    getSummary: (selection: SelectionQuerySpec) =>
      invokeTyped('get_selection_summary', { selection } as never) as Promise<SelectionSummary>,
    addTags: (selection: SelectionQuerySpec, tagStrings: string[]) =>
      invokeTyped('add_tags_selection', { selection, tag_strings: tagStrings } as never),
    removeTags: (selection: SelectionQuerySpec, tagStrings: string[]) =>
      invokeTyped('remove_tags_selection', { selection, tag_strings: tagStrings } as never),
    updateRating: (selection: SelectionQuerySpec, rating: number | null) =>
      invokeTyped('update_rating_selection', { selection, rating } as never),
    setNotes: (selection: SelectionQuerySpec, notes: Record<string, string>) =>
      invokeTyped('set_notes_selection', { selection, notes } as never),
    setSourceUrls: (selection: SelectionQuerySpec, urls: string[]) =>
      invokeTyped('set_source_urls_selection', { selection, urls } as never),
  },

  folders: {
    list: () =>
      invokeTyped('list_folders') as Promise<Folder[]>,
    create: (params: { name: string; parent_id?: number | null; icon?: string; color?: string }) =>
      invokeTyped('create_folder', params as never) as Promise<Folder>,
    update: (params: { folder_id: number; name?: string; icon?: string; color?: string; auto_tags?: string[] }) =>
      invokeTyped('update_folder', params as never) as unknown as Promise<void>,
    delete: (folderId: number) =>
      invokeTyped('delete_folder', { folder_id: folderId }) as unknown as Promise<void>,
    updateParent: (folderId: number, newParentId?: number | null) =>
      invokeTyped('update_folder_parent', { folder_id: folderId, new_parent_id: newParentId } as never) as unknown as Promise<void>,
    // PBI-057: Atomic move_folder — reparent + reorder in one transaction.
    moveFolder: (folderId: number, newParentId: number | null, siblingOrder: [number, number][]) =>
      invokeTyped('move_folder', { folder_id: folderId, new_parent_id: newParentId, sibling_order: siblingOrder }) as unknown as Promise<void>,
    addFile: (folderId: number, hash: string) =>
      invokeTyped('add_file_to_folder', { folder_id: folderId, hash }) as unknown as Promise<void>,
    // PBI-054: Batch add files to folder.
    addFilesBatch: (folderId: number, hashes: string[]) =>
      invokeTyped('add_files_to_folder_batch', { folder_id: folderId, hashes }),
    removeFile: (folderId: number, hash: string) =>
      invokeTyped('remove_file_from_folder', { folder_id: folderId, hash }) as unknown as Promise<void>,
    removeFilesBatch: (folderId: number, hashes: string[]) =>
      invokeTyped('remove_files_from_folder_batch', { folder_id: folderId, hashes }),
    getFiles: (folderId: number) =>
      invokeTyped('get_folder_files', { folder_id: folderId }),
    getCoverHash: (folderId: number) =>
      invokeTyped('get_folder_cover_hash', { folder_id: folderId }),
    getFileFolders: (hash: string) =>
      invokeTyped('get_file_folders', { hash }) as Promise<FolderMembership[]>,
    getEntityFolders: (entityId: number) =>
      invokeTyped('get_entity_folders', { entity_id: entityId }) as Promise<FolderMembership[]>,
    reorder: (moves: [number, number][]) =>
      invokeTyped('reorder_folders', { moves }) as unknown as Promise<void>,
    reorderItems: (folderId: number, moves: FolderReorderMove[]) =>
      invokeTyped('reorder_folder_items', { folder_id: folderId, moves } as never) as unknown as Promise<void>,
    sortItems: (folderId: number, sortBy: string, direction: string, hashes?: string[]) =>
      invokeTyped('sort_folder_items', { folder_id: folderId, sort_by: sortBy, direction, hashes } as never) as unknown as Promise<void>,
    reverseItems: (folderId: number, hashes?: string[]) =>
      invokeTyped('reverse_folder_items', { folder_id: folderId, hashes } as never) as unknown as Promise<void>,
  },

  smartFolders: {
    list: async (): Promise<SmartFolder[]> => {
      const raw = await invokeTyped('list_smart_folders') as Array<Record<string, unknown>>;
      return raw.map(normalizeSmartFolder);
    },
    create: async (folder: SmartFolder): Promise<SmartFolder> => {
      const raw = await invokeTyped('create_smart_folder', { folder } as never) as Record<string, unknown>;
      return normalizeSmartFolder(raw);
    },
    update: async (id: string, folder: SmartFolder): Promise<SmartFolder> => {
      const raw = await invokeTyped('update_smart_folder', { id, folder } as never) as Record<string, unknown>;
      return normalizeSmartFolder(raw);
    },
    delete: (id: string) =>
      invokeTyped('delete_smart_folder', { id }) as unknown as Promise<void>,
    count: (predicate: SmartFolderPredicate) =>
      invokeTyped('count_smart_folder', { predicate } as never) as Promise<number>,
    reorder: (moves: [number, number][]) =>
      invokeTyped('reorder_smart_folders', { moves }) as unknown as Promise<void>,
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
        preferred_hash: null,
      } as never) as Promise<SmartMergeResult | Record<string, string>>,
    getCount: () =>
      invokeTyped('get_duplicate_count') as Promise<{ count: number }>,
    scan: () =>
      invokeTyped('scan_duplicates', { threshold: null } as never) as Promise<ScanDuplicatesResult>,
    getSettings: () =>
      invokeTyped('get_duplicate_settings') as Promise<DuplicateSettings>,
    updateSettings: (settings: Partial<DuplicateSettings>) =>
      invokeTyped('update_duplicate_settings', settings as never) as Promise<{ ok: boolean }>,
  },

  subscriptions: {
    list: () =>
      invokeTyped('get_subscriptions') as Promise<SubscriptionInfo[]>,
    create: (params: {
      name: string;
      site_id: string;
      queries: string[];
      flow_id?: number;
      initial_file_limit?: number;
      periodic_file_limit?: number;
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
    pauseQuery: (id: string, paused: boolean) =>
      invokeTyped('pause_subscription_query', { id, paused }) as unknown as Promise<void>,
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
  },

  flows: {
    list: () =>
      invokeTyped('get_flows') as Promise<FlowInfo[]>,
    create: (name: string, schedule?: string) =>
      invokeTyped('create_flow', { name, schedule: schedule ?? null } as never) as Promise<FlowInfo>,
    delete: (id: string, deleteFiles?: boolean) =>
      invokeTyped('delete_flow', { id, delete_files: deleteFiles ?? null } as never) as unknown as Promise<void>,
    rename: (id: string, name: string) =>
      invokeTyped('rename_flow', { id, name }) as unknown as Promise<void>,
    setSchedule: (id: string, schedule: string) =>
      invokeTyped('set_flow_schedule', { id, schedule }) as unknown as Promise<void>,
    run: (id: string) =>
      invokeTyped('run_flow', { id }) as unknown as Promise<void>,
    stop: (id: string) =>
      invokeTyped('stop_flow', { id }) as unknown as Promise<void>,
  },

  ptr: {
    getStatus: () =>
      invokeTyped('get_ptr_status') as Promise<PtrStats>,
    isSyncing: () =>
      invokeTyped('is_ptr_syncing') as Promise<boolean>,
    getSyncProgress: () =>
      invokeTyped('get_ptr_sync_progress') as Promise<PtrSyncProgress | null>,
    sync: () =>
      invokeTyped('ptr_sync') as Promise<{ id: string; message: string }>,
    cancelSync: () =>
      invokeTyped('cancel_ptr_sync') as unknown as Promise<void>,
    cancelBootstrap: () =>
      invokeTyped('ptr_cancel_bootstrap') as unknown as Promise<void>,
    bootstrapFromSnapshot: (req: { snapshot_dir: string; ptr_service_id?: number | null; mode: string }) =>
      invokeTyped('ptr_bootstrap_from_hydrus_snapshot', req) as Promise<Record<string, unknown>>,
    getBootstrapStatus: () =>
      invokeTyped('ptr_get_bootstrap_status') as Promise<PtrBootstrapStatus>,
    getCompactIndexStatus: () =>
      invokeTyped('ptr_get_compact_index_status') as Promise<PtrCompactIndexStatus>,
    getNamespaceSummary: () =>
      invokeTyped('ptr_get_namespace_summary') as Promise<NamespaceSummary[]>,
    getTagsPaginated: (params: { namespace?: string; search?: string; cursor?: string; limit?: number }) =>
      invokeTyped('ptr_get_tags_paginated', params as never) as Promise<TagRecord[]>,
    getTagSiblings: (tagId: number) =>
      invokeTyped('ptr_get_tag_siblings', { tag_id: tagId }) as Promise<TagRelation[]>,
    getTagParents: (tagId: number) =>
      invokeTyped('ptr_get_tag_parents', { tag_id: tagId }) as Promise<TagRelation[]>,
    getSyncPerfBreakdown: () =>
      invokeTyped('get_ptr_sync_perf_breakdown') as Promise<PtrSyncPerfBreakdown>,
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

  stats: {
    getImageStorageStats: () =>
      invokeTyped('get_image_storage_stats') as Promise<FileStats>,
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
    enableModernWindowStyle: (cornerRadius: number) =>
      invokeTyped('enable_modern_window_style', { cornerRadius }) as unknown as Promise<void>,
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
    setRating: (id: number, rating: number | null) =>
      invokeTyped('set_collection_rating', { id, rating }) as unknown as Promise<void>,
    setSourceUrls: (id: number, sourceUrls: string[]) =>
      invokeTyped('set_collection_source_urls', { id, source_urls: sourceUrls }) as unknown as Promise<void>,
    reorderMembers: (id: number, hashes: string[]) =>
      invokeTyped('reorder_collection_members', { id, hashes }) as unknown as Promise<void>,
    create: (params: { name: string; description?: string | null; tags?: string[] }) =>
      invokeTyped('create_collection', params as never),
    addMembers: (params: { id: number; hashes: string[] }) =>
      invokeTyped('add_collection_members', params),
    removeMembers: (params: { id: number; hashes: string[] }) =>
      invokeTyped('remove_collection_members', params),
    update: (params: { id: number; name?: string; description?: string | null; tags?: string[]; sourceUrls?: string[] }) =>
      invokeTyped('update_collection', {
        id: params.id,
        name: params.name,
        description: params.description,
        tags: params.tags,
        source_urls: params.sourceUrls,
      } as never) as unknown as Promise<void>,
    delete: (id: number) =>
      invokeTyped('delete_collection', { id }) as unknown as Promise<void>,
  },

  companion: {
    getNamespaceValues: (namespace: string) =>
      invokeTyped('companion_get_namespace_values', { namespace }) as Promise<CompanionNamespaceValue[]>,
    getFilesByTag: (tag: string) =>
      invokeTyped('companion_get_files_by_tag', { tag }) as Promise<EntitySlim[]>,
  },
};
