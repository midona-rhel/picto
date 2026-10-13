/**
 * IPC argument contract recorder.
 *
 * Calls every exported platform API function with representative arguments
 * and records the exact `[command, args]` payloads the renderer sends. The
 * recording is committed as `core/tests/fixtures/ipc_payloads.json`, and the
 * Rust test `core/tests/ipc_contract.rs` feeds each payload through the real
 * command dispatcher, so a renamed or reshaped argument fails CI on the side
 * that did not change.
 *
 * After adding or changing a command, regenerate the fixture with
 * `npx vitest run src/platform/ipcContract.test.ts -u` and commit it.
 */
import { describe, expect, it, vi } from 'vitest';
import { invoke } from './ipc';
import * as aiTaggerApi from './aiTaggerApi';
import * as associatedFilesApi from './associatedFilesApi';
import * as duplicateApi from './duplicateApi';
import * as entityApi from './entityApi';
import * as folderApi from './folderApi';
import * as historyApi from './historyApi';
import * as mediaFormatApi from './mediaFormatApi';
import * as navigationApi from './navigationApi';
import * as pictoPackApi from './pictoPackApi';
import * as settingsApi from './settingsApi';
import * as shellApi from './shellApi';
import * as smartFolderApi from './smartFolderApi';
import * as subscriptionApi from './subscriptionApi';
import * as tagApi from './tagApi';
import * as taskApi from './taskApi';
import * as updateApi from './updateApi';
import { setTargetNotes, setTargetSourceUrls } from '../controllers/entityMutations';
import { compileGridQuery, createEmptyItemFilters } from '../shared/lib/itemFilters';
import type { BaseScope, EntityTarget, EntityViewQuery, ViewQuerySpec } from '../shared/types/canonical';

vi.mock('./ipc', () => ({
  invoke: vi.fn(),
}));

const MODULES = {
  aiTaggerApi,
  associatedFilesApi,
  duplicateApi,
  entityApi,
  folderApi,
  historyApi,
  mediaFormatApi,
  navigationApi,
  pictoPackApi,
  settingsApi,
  shellApi,
  smartFolderApi,
  subscriptionApi,
  tagApi,
  taskApi,
  updateApi,
} as const;

type ModuleName = keyof typeof MODULES;

/** Exports that never reach `invoke`, so they have no Rust payload to check. */
const NOT_IPC: Record<string, string> = {
  'associatedFilesApi.claimAssociatedPictoPack': 'Electron preload bridge',
  'settingsApi.viewPrefsToPatch': 'pure mapping helper',
  'shellApi.shellShowInFolder': 'Electron shell bridge',
  'shellApi.shellOpenPath': 'Electron shell bridge',
  'shellApi.getOpenWithOptions': 'Electron shell bridge',
  'shellApi.shellOpenWithApplication': 'Electron shell bridge',
  'shellApi.shellOpenWithChooser': 'Electron shell bridge',
  'shellApi.clipboardWriteText': 'Electron clipboard bridge',
  'shellApi.clipboardCopyFile': 'Electron clipboard bridge',
  'shellApi.clipboardCopyFiles': 'Electron clipboard bridge',
  'shellApi.hasClipboardImport': 'Electron clipboard bridge',
  'shellApi.readClipboardImport': 'Electron clipboard bridge',
  'updateApi.getUpdateState': 'Electron update service bridge',
  'updateApi.checkForUpdates': 'Electron update service bridge',
  'updateApi.installUpdate': 'Electron update service bridge',
  'updateApi.openUpdateRelease': 'Electron update service bridge',
  'updateApi.acknowledgeInstalledUpdate': 'Electron update service bridge',
  'updateApi.onUpdateState': 'Electron update service bridge',
};

/** Rust rebases this prefix into its temp directory (see core/tests/ipc_contract.rs). */
const FILES = '/picto-contract';
const HASH_A = 'a'.repeat(64);
const HASH_B = 'b'.repeat(64);

const sort = { field: 'imported_at', direction: 'descending', random_seed: null } as const;
const allQuery: EntityViewQuery = compileGridQuery({ kind: 'all' }, createEmptyItemFilters(), sort);
const filteredQuery: EntityViewQuery = compileGridQuery(
  { kind: 'folder', folder_id: 3 },
  {
    ...createEmptyItemFilters(),
    include_tags: [{ tag_id: 4, name: 'cat' }],
    exclude_tags: [{ tag_id: 5, name: 'dog' }],
    tag_match_mode: 'all',
    include_folder_ids: [6],
    exclude_folder_ids: [7],
    ratings: [0, 5],
    include_mime_types: ['image/*', 'video/mp4'],
    exclude_mime_types: ['image/gif'],
    text: 'sunset',
    color_hex: '#ff8800',
    imported_after: '2024-01-01T00:00:00Z',
    imported_before: '2024-12-31T00:00:00Z',
    modified_after: '2024-02-01T00:00:00Z',
    min_duration_ms: BigInt(1000),
    max_size_bytes: BigInt(1_048_576),
    min_width: BigInt(640),
    max_height: BigInt(2160),
  },
  { field: 'random', direction: 'ascending', random_seed: 'seed-1' },
  '',
  true,
);
const scopes: BaseScope[] = [
  { kind: 'inbox' },
  { kind: 'trash' },
  { kind: 'media_matches', item_id: 1 },
  { kind: 'recently_viewed' },
  { kind: 'untagged' },
  { kind: 'uncategorized' },
  { kind: 'smart_folder', smart_folder_id: 2 },
];
const scopeQueries: EntityViewQuery[] = scopes.map((scope) => compileGridQuery(scope, createEmptyItemFilters(), {
  field: 'folder_order',
  direction: 'ascending',
  random_seed: null,
}));

const explicitTarget: EntityTarget = { kind: 'explicit', root_ids: [1, 2] };
const emptyTarget: EntityTarget = { kind: 'explicit', root_ids: [] };
const queryTarget: EntityTarget = { kind: 'query', query: filteredQuery, excluded_root_ids: [2] };
const rangeTarget: EntityTarget = {
  kind: 'range', query: allQuery,
  spans: [{ anchor_root_id: 1, focus_root_id: 3, anchor_index: 0, focus_index: 2 }],
  included_root_ids: [4], excluded_root_ids: [2],
};
const ALL_TARGETS = [explicitTarget, emptyTarget, queryTarget, rangeTarget];

const smartFolderView: ViewQuerySpec = {
  filter: {
    kind: 'any',
    value: [
      { kind: 'clause', value: { clause: 'notes_present', present: true } },
      { kind: 'clause', value: { clause: 'source_urls_present', present: false } },
      { kind: 'clause', value: { clause: 'captured_at', minimum_ms: 0, maximum_ms: null } },
      { kind: 'clause', value: { clause: 'text', field: 'notes', query: 'todo' } },
      { kind: 'not', value: { kind: 'clause', value: { clause: 'ratings', ratings: ['one'] } } },
    ],
  },
  sort: { field: 'name', direction: 'ascending', random_seed: null },
};

const subscriptionView = {
  subscription_id: 1,
  name: 'Artist',
  schedule: 'daily',
  paused: false,
  status: 'running',
  active_run_id: 9,
  root_count: 0,
  periodic_post_limit: 100,
  initial_post_limit: 100,
  destination: { target_folder_ids: [], automatic_tags: [] },
  progress: {
    discovered: 0,
    posts_traversed: 0,
    posts_added: 0,
    posts_skipped: 0,
    files_already_in_library: 0,
    downloaded: 0,
    ingested: 0,
    failed: 0,
  },
  cover_file_hash: null,
  open_issue_count: 0,
  queries: [{
    query_id: 2,
    site_id: 'danbooru',
    query_kind: 'tag',
    query_text: 'cat',
    display_name: null,
    notes: null,
    group_posts: true,
    paused: false,
    last_success_at: null,
    last_failure_at: null,
    last_failure_kind: null,
    last_failure_message: null,
    media_count: 0,
    post_count: 0,
    initial_run_complete: false,
    source_history_complete: false,
    successful_run_count: 0,
  }],
};

/** Minimal backend answers so callers that read the result keep going. */
function fakeResponse(command: string): unknown {
  switch (command) {
    case 'settings.get':
    case 'settings.view.get':
      return { value: {}, revision: 0 };
    case 'subscriptions.list':
      return { subscriptions: [subscriptionView], global_paused: false };
    case 'subscriptions.create':
      return { subscription_id: 1 };
    case 'subscriptions.queries.add':
      return { query_id: 2 };
    case 'subscriptions.runs.list':
      return { runs: [] };
    case 'subscriptions.issues.list':
      return { issues: [], next_cursor: null, total_count: 0 };
    case 'media.resolve_paths':
      return [{ file_hash: HASH_A, path: '/library/a.png' }];
    case 'items.query':
      return { items: [], next_cursor: null, total: 0, media_count: 0, total_size_bytes: 0, revision: 0 };
    case 'duplicates.list':
    case 'sources.list':
    case 'auth.credentials.list':
    case 'auth.health.list':
    case 'media.formats.list':
      return [];
    case 'folders.cover.get':
    case 'subscriptions.progress.get':
    case 'duplicates.resolve_automatically':
    case 'history.state':
      return null;
    default:
      return {};
  }
}

const duplicatePair: duplicateApi.DuplicatePair = {
  file_id_a: 11,
  file_id_b: 12,
  status: 'detected',
} as duplicateApi.DuplicatePair;

/** One entry per exported platform function; several calls cover edge values. */
const CASES: Record<string, () => unknown> = {
  'aiTaggerApi.aiTaggerStatus': () => aiTaggerApi.aiTaggerStatus(),
  'aiTaggerApi.aiTaggerDownloadModel': () => aiTaggerApi.aiTaggerDownloadModel('wd14'),
  'aiTaggerApi.aiTaggerCancelDownload': () => aiTaggerApi.aiTaggerCancelDownload('wd14'),
  'aiTaggerApi.aiTaggerDeleteModel': () => aiTaggerApi.aiTaggerDeleteModel('wd14'),
  'aiTaggerApi.aiTaggerOptimizeModel': () => aiTaggerApi.aiTaggerOptimizeModel('wd14'),
  'aiTaggerApi.aiTagPredict': async () => {
    await aiTaggerApi.aiTagPredict([{ rootId: 1, mediaItemId: 10 }]);
    await aiTaggerApi.aiTagPredict([], ['wd14', 'e621']);
  },
  'aiTaggerApi.aiTaggerUnload': () => aiTaggerApi.aiTaggerUnload(),
  'aiTaggerApi.aiTagApply': async () => {
    await aiTaggerApi.aiTagApply([{ root_id: 1, tags: ['cat', 'character:alice'] }]);
    await aiTaggerApi.aiTagApply([]);
  },

  'duplicateApi.scanDuplicates': async () => {
    await duplicateApi.scanDuplicates();
    await duplicateApi.scanDuplicates(8);
  },
  'duplicateApi.getDuplicatePairs': () => duplicateApi.getDuplicatePairs(),
  'duplicateApi.getDuplicateItemDetails': () => duplicateApi.getDuplicateItemDetails(1),
  'duplicateApi.resolveDuplicatePair': async () => {
    for (const action of ['smart_merge', 'keep_left', 'keep_right', 'keep_both', 'not_duplicate'] as const) {
      await duplicateApi.resolveDuplicatePair(action, duplicatePair);
    }
  },

  'entityApi.queryItems': async () => {
    await entityApi.queryItems(allQuery, { cursor: null, limit: 100 });
    await entityApi.queryItems(filteredQuery, { cursor: 'opaque-cursor', limit: 1 });
    for (const query of scopeQueries) await entityApi.queryItems(query, { limit: 1 });
  },
  'entityApi.queryItemWindow': async () => {
    await entityApi.queryItemWindow(allQuery, { position: { kind: 'ordinal', value: 0 }, limit: 200 });
    await entityApi.queryItemWindow(
      filteredQuery,
      { position: { kind: 'fraction', value: 0.5 }, limit: 50 },
      { channel: 'grid', generation: 3 },
    );
  },
  'entityApi.supersedeItemWindow': () => entityApi.supersedeItemWindow({ channel: 'grid', generation: 4 }),
  'entityApi.getRootPositions': () => entityApi.getRootPositions(filteredQuery, [1, 2]),
  'entityApi.getRootMembership': () => entityApi.getRootMembership(filteredQuery, [1, 2]),
  'entityApi.queryMimeFacets': () => entityApi.queryMimeFacets(filteredQuery),
  'entityApi.recordMediaView': () => entityApi.recordMediaView(1),
  'entityApi.clearRecentViews': () => entityApi.clearRecentViews(),
  'entityApi.renameItem': () => entityApi.renameItem(1, 'Renamed'),
  'entityApi.renameItems': async () => {
    await entityApi.renameItems([{ root_id: 1, name: 'One' }, { root_id: 2, name: 'Two' }]);
    await entityApi.renameItems([]);
  },
  'entityApi.patchMediaEntities': async () => {
    for (const target of ALL_TARGETS) await entityApi.patchMediaEntities(target, { rating: 'four' });
    await entityApi.patchMediaEntities(explicitTarget, { notes: 'A note' });
    await entityApi.patchMediaEntities(explicitTarget, { notes: null });
    await entityApi.patchMediaEntities(explicitTarget, { source_urls: ['https://example.com/post/1'] });
    await entityApi.patchMediaEntities(explicitTarget, { source_urls: [] });
    // Controller paths that build the patch: blank notes clear them.
    await setTargetNotes(explicitTarget, '   ');
    await setTargetNotes(explicitTarget, ' Trimmed ');
    await setTargetSourceUrls(queryTarget, []);
  },
  'entityApi.applyEntityTags': async () => {
    await entityApi.applyEntityTags(explicitTarget, 'add', ['cat', 'character:alice']);
    await entityApi.applyEntityTags(queryTarget, 'remove', ['cat']);
    await entityApi.applyEntityTags(rangeTarget, 'add', []);
  },
  'entityApi.setItemLifecycle': async () => {
    await entityApi.setItemLifecycle(explicitTarget, 'trash');
    await entityApi.setItemLifecycle(queryTarget, 'active');
    await entityApi.setItemLifecycle(rangeTarget, 'inbox');
  },
  'entityApi.deleteItems': async () => {
    await entityApi.deleteItems(explicitTarget);
    await entityApi.deleteItems(queryTarget);
  },
  'entityApi.rejectDeleteItems': () => entityApi.rejectDeleteItems(explicitTarget),
  'entityApi.getSelectionSummary': async () => {
    for (const target of ALL_TARGETS) await entityApi.getSelectionSummary(target);
  },
  'entityApi.resolveImageSelection': () => entityApi.resolveImageSelection(rangeTarget),
  'entityApi.getCollectionNoteDraft': () => entityApi.getCollectionNoteDraft(explicitTarget),
  'entityApi.organizeIntoGroup': async () => {
    await entityApi.organizeIntoGroup({
      target: explicitTarget,
      cover_root_id: 1,
      winning_collection_id: null,
      name: null,
      notes: null,
    });
    await entityApi.organizeIntoGroup({
      target: queryTarget,
      cover_root_id: 1,
      winning_collection_id: 5,
      name: 'Set',
      notes: 'Collected',
    });
  },
  'entityApi.detachItems': async () => {
    await entityApi.detachItems({ collection_id: 5, media_ids: [10, 11], target_lifecycle: null });
    await entityApi.detachItems({ collection_id: 5, media_ids: [10], target_lifecycle: 'inbox' });
  },
  'entityApi.ungroup': () => entityApi.ungroup(5),
  'entityApi.reorderGroup': () => entityApi.reorderGroup({ collection_id: 5, media_ids: [11, 10] }),

  'folderApi.createFolder': async () => {
    await folderApi.createFolder({ name: 'Top' });
    await folderApi.createFolder({ name: 'Child', parent_id: 3 });
  },
  'folderApi.duplicateFolder': () => folderApi.duplicateFolder(3),
  'folderApi.deleteFolders': async () => {
    await folderApi.deleteFolders([3, 4]);
    await folderApi.deleteFolders([]);
  },
  'folderApi.removeEntitiesFromFolder': () => folderApi.removeEntitiesFromFolder(3, explicitTarget),
  'folderApi.renameFolder': () => folderApi.renameFolder(3, 'Renamed'),
  'folderApi.setFolderMetadata': async () => {
    await folderApi.setFolderMetadata({ folder_id: 3, icon: 'star', color: '#ff0000', notes: 'Notes' });
    await folderApi.setFolderMetadata({ folder_id: 3, icon: null, color: null, notes: null });
  },
  'folderApi.getFolderAutoTags': () => folderApi.getFolderAutoTags(3),
  'folderApi.setFolderAutoTags': async () => {
    await folderApi.setFolderAutoTags(3, ['cat']);
    await folderApi.setFolderAutoTags(3, []);
  },
  'folderApi.moveFolder': async () => {
    await folderApi.moveFolder(4, 3);
    await folderApi.moveFolder(4, null);
  },
  'folderApi.reorderFolderChildren': async () => {
    await folderApi.reorderFolderChildren(3, [5, 4]);
    await folderApi.reorderFolderChildren(null, [3]);
  },
  'folderApi.sortFolderTree': () => folderApi.sortFolderTree(3, true, false),
  'folderApi.updateFolderMembership': async () => {
    await folderApi.updateFolderMembership(explicitTarget, 3, 'add');
    await folderApi.updateFolderMembership(rangeTarget, 3, 'remove');
  },
  'folderApi.setFolderWatchConfig': () => folderApi.setFolderWatchConfig(3, `${FILES}/watched`, true, 'inbox'),
  'folderApi.clearFolderWatchConfig': () => folderApi.clearFolderWatchConfig(3),
  'folderApi.getFolderCover': () => folderApi.getFolderCover(3),
  'folderApi.setFolderCover': () => folderApi.setFolderCover(3, 1),
  'folderApi.addMedia': async () => {
    await folderApi.addMedia([`${FILES}/import/a.png`], { lifecycle: 'inbox' });
    await folderApi.addMedia([`${FILES}/import/dir`], {
      tags: ['cat'],
      source_urls: ['https://example.com'],
      lifecycle: 'active',
      parent_folder_id: 3,
      preserve_structure: true,
      include_subfolders: false,
      include_folders_without_media: true,
      watch_source_folder: true,
      delete_after_ingest: false,
      group_files: true,
    });
  },
  'folderApi.analyzeFolderTree': () => folderApi.analyzeFolderTree({
    path: `${FILES}/import/dir`,
    destination_folder_id: null,
    include_subfolders: true,
    include_source_root: false,
  }),
  'folderApi.exportMedia': async () => {
    await folderApi.exportMedia(explicitTarget, { output_dir: `${FILES}/export` });
    await folderApi.exportMedia(queryTarget, {
      output_dir: `${FILES}/export`,
      format: 'webp',
      quality: 80,
      width: 1024,
      height: null,
      keep_aspect: false,
    });
  },
  'folderApi.reorderFolderMembers': async () => {
    await folderApi.reorderFolderMembers(3, [2, 1]);
    await folderApi.reorderFolderMembers(3, []);
  },
  'folderApi.sortFolderItems': async () => {
    for (const field of ['name', 'imported_at', 'created_at', 'modified_at', 'size', 'notes'] as const) {
      await folderApi.sortFolderItems(3, field);
    }
  },

  'historyApi.getHistoryState': () => historyApi.getHistoryState(),
  'historyApi.undoHistory': () => historyApi.undoHistory(),
  'historyApi.redoHistory': () => historyApi.redoHistory(),

  'mediaFormatApi.listAcceptedMediaFormats': () => mediaFormatApi.listAcceptedMediaFormats(),

  'navigationApi.getNavigation': () => navigationApi.getNavigation(),
  'navigationApi.getSidebarCounts': () => navigationApi.getSidebarCounts(),

  'pictoPackApi.inspectPictoPack': () => pictoPackApi.inspectPictoPack(`${FILES}/packs/in.pictopack`),
  'pictoPackApi.exportPictoPack': async () => {
    await pictoPackApi.exportPictoPack({ kind: 'items', target: explicitTarget }, `${FILES}/packs/items.pictopack`);
    await pictoPackApi.exportPictoPack({ kind: 'folder', folder_id: 3 }, `${FILES}/packs/folder.pictopack`);
    await pictoPackApi.exportPictoPack({ kind: 'smart_folder', smart_folder_id: 2 }, `${FILES}/packs/smart.pictopack`);
  },
  'pictoPackApi.importPictoPack': () => pictoPackApi.importPictoPack(`${FILES}/packs/in.pictopack`),

  'settingsApi.getSettingsSnapshot': () => settingsApi.getSettingsSnapshot(),
  'settingsApi.getSettings': () => settingsApi.getSettings(),
  'settingsApi.patchSettings': async () => {
    await settingsApi.patchSettings({ gridSpacing: 'tight', zoomFactor: null, starredTags: [] });
    await settingsApi.patchSettings({});
  },
  'settingsApi.replaceSettings': async () => {
    const settings = await settingsApi.getSettings();
    await settingsApi.replaceSettings(settings);
  },
  'settingsApi.getViewPrefsSnapshot': () => settingsApi.getViewPrefsSnapshot(settingsApi.GRID_DEFAULTS_SCOPE),
  'settingsApi.getViewPrefs': () => settingsApi.getViewPrefs('folder:3'),
  'settingsApi.setViewPrefs': async () => {
    await settingsApi.setViewPrefs('folder:3', { sort_field: 'name', target_size: 180, show_name: null });
    const prefs = await settingsApi.getViewPrefs('folder:3');
    await settingsApi.setViewPrefs(settingsApi.GRID_DEFAULTS_SCOPE, settingsApi.viewPrefsToPatch(prefs));
  },
  'settingsApi.resetViewPrefs': () => settingsApi.resetViewPrefs(),

  'shellApi.openExternalUrl': () => shellApi.openExternalUrl('https://example.com'),
  'shellApi.openSettingsWindow': async () => {
    await shellApi.openSettingsWindow();
    await shellApi.openSettingsWindow('subscriptions');
  },
  'shellApi.resolveFilePath': () => shellApi.resolveFilePath(HASH_A),
  'shellApi.reverseImageSearch': () => shellApi.reverseImageSearch(HASH_B, 'saucenao'),
  'shellApi.openDetailWindow': async () => {
    await shellApi.openDetailWindow({ hash: HASH_A, width: 800, height: 600 });
    await shellApi.openDetailWindow({ item_id: 5 });
  },
  'shellApi.resolveTargetFilePaths': () => shellApi.resolveTargetFilePaths(queryTarget),
  'shellApi.regenerateThumbnailsBatch': () => shellApi.regenerateThumbnailsBatch([HASH_A, HASH_B]),
  'shellApi.setThumbnail': () => shellApi.setThumbnail(HASH_A, 'iVBORw0KGgo='),

  'smartFolderApi.createSmartFolder': async () => {
    await smartFolderApi.createSmartFolder({
      name: 'Smart', parent_id: null, icon: null, color: null, notes: null, view: smartFolderView,
    });
    await smartFolderApi.createSmartFolder({
      name: 'Nested', parent_id: 2, icon: 'star', color: '#00ff00', notes: 'n', view: filteredQuery.view,
    });
  },
  'smartFolderApi.deleteSmartFolder': () => smartFolderApi.deleteSmartFolder(2),
  'smartFolderApi.updateSmartFolder': () => smartFolderApi.updateSmartFolder(2, {
    name: 'Smart', parent_id: null, icon: null, color: null, notes: null, view: allQuery.view,
  }),
  'smartFolderApi.moveSmartFolder': async () => {
    await smartFolderApi.moveSmartFolder(2, 8);
    await smartFolderApi.moveSmartFolder(2, null);
  },
  'smartFolderApi.reorderSmartFolders': async () => {
    await smartFolderApi.reorderSmartFolders(null, [2, 8]);
    await smartFolderApi.reorderSmartFolders(8, []);
  },

  'subscriptionApi.getSubscriptionOverview': () => subscriptionApi.getSubscriptionOverview(),
  'subscriptionApi.getSubscriptionSites': () => subscriptionApi.getSubscriptionSites(),
  'subscriptionApi.getSubscriptionCovers': () => subscriptionApi.getSubscriptionCovers(),
  'subscriptionApi.getSubscriptionCoverCandidates': async () => {
    await subscriptionApi.getSubscriptionCoverCandidates('1');
    await subscriptionApi.getSubscriptionCoverCandidates('1', { imported_at: '2024-01-01T00:00:00Z', media_item_id: 10 }, 50);
  },
  'subscriptionApi.setSubscriptionCover': () => subscriptionApi.setSubscriptionCover('1', {
    // Focus is permille (0-1000), as SubscriptionCoverDialog sends it.
    media_item_id: 10, focus_x: 250, focus_y: 750, zoom_percent: 160,
  }),
  'subscriptionApi.getSubscriptions': () => subscriptionApi.getSubscriptions(),
  'subscriptionApi.setSubscriptionSchedule': () => subscriptionApi.setSubscriptionSchedule('1', 'weekly'),
  'subscriptionApi.setSubscriptionPostsPerRun': () => subscriptionApi.setSubscriptionPostsPerRun('1', 250),
  'subscriptionApi.setSubscriptionDestination': async () => {
    await subscriptionApi.setSubscriptionDestination('1', { target_folder_ids: [3], automatic_tags: ['cat'] });
    await subscriptionApi.setSubscriptionDestination('1', { target_folder_ids: [], automatic_tags: [] });
  },
  'subscriptionApi.createSubscription': () => subscriptionApi.createSubscription({ name: 'Artist' }),
  'subscriptionApi.deleteSubscription': () => subscriptionApi.deleteSubscription('1'),
  'subscriptionApi.resetSubscription': () => subscriptionApi.resetSubscription('1'),
  'subscriptionApi.renameSubscription': () => subscriptionApi.renameSubscription('1', 'Renamed'),
  'subscriptionApi.setSubscriptionHold': () => subscriptionApi.setSubscriptionHold('1', true),
  'subscriptionApi.pauseAllSubscriptions': () => subscriptionApi.pauseAllSubscriptions(false),
  'subscriptionApi.runSubscription': () => subscriptionApi.runSubscription('1'),
  'subscriptionApi.pauseSubscriptionRun': () => subscriptionApi.pauseSubscriptionRun('1'),
  'subscriptionApi.resumeSubscriptionRun': () => subscriptionApi.resumeSubscriptionRun('1'),
  'subscriptionApi.runSubscriptionQuery': () => subscriptionApi.runSubscriptionQuery('2'),
  'subscriptionApi.startGalleryImport': () => subscriptionApi.startGalleryImport('https://e-hentai.org/g/123456/0123456789/'),
  'subscriptionApi.cleanupGalleryImport': () => subscriptionApi.cleanupGalleryImport('1'),
  'subscriptionApi.stopSubscription': () => subscriptionApi.stopSubscription('1'),
  'subscriptionApi.addSubscriptionQuery': async () => {
    await subscriptionApi.addSubscriptionQuery('1', 'danbooru', 'cat');
    await subscriptionApi.addSubscriptionQuery('1', 'danbooru', 'dog', 'with notes');
  },
  'subscriptionApi.editSubscriptionQuery': async () => {
    await subscriptionApi.editSubscriptionQuery(2, 'danbooru', 'cat');
    await subscriptionApi.editSubscriptionQuery(2, 'danbooru', 'cat', 'Cats', 'notes');
  },
  'subscriptionApi.deleteSubscriptionQuery': () => subscriptionApi.deleteSubscriptionQuery('2'),
  'subscriptionApi.pauseSubscriptionQuery': () => subscriptionApi.pauseSubscriptionQuery('2', true),
  'subscriptionApi.setSubscriptionQueryGrouping': () => subscriptionApi.setSubscriptionQueryGrouping('2', false),
  'subscriptionApi.getRunningSubscriptions': () => subscriptionApi.getRunningSubscriptions(),
  'subscriptionApi.getRunningSubscriptionProgress': () => subscriptionApi.getRunningSubscriptionProgress(),
  'subscriptionApi.getSubscriptionProgress': () => subscriptionApi.getSubscriptionProgress({ id: '1', name: 'Artist' }),
  'subscriptionApi.listSubscriptionRuns': async () => {
    await subscriptionApi.listSubscriptionRuns('1');
    await subscriptionApi.listSubscriptionRuns('1', 5);
  },
  'subscriptionApi.listSubscriptionIssues': async () => {
    await subscriptionApi.listSubscriptionIssues('1');
    await subscriptionApi.listSubscriptionIssues('1', '2', 10, { last_seen_at: '2024-01-01T00:00:00Z', issue_id: 7 });
  },
  'subscriptionApi.acknowledgeSubscriptionIssues': () => subscriptionApi.acknowledgeSubscriptionIssues('1'),
  'subscriptionApi.getSubscriptionRunActivity': () => subscriptionApi.getSubscriptionRunActivity(9),
  'subscriptionApi.listCredentials': () => subscriptionApi.listCredentials(),
  'subscriptionApi.listCredentialHealth': () => subscriptionApi.listCredentialHealth(),
  'subscriptionApi.deleteCredential': () => subscriptionApi.deleteCredential('danbooru'),
  'subscriptionApi.startAuthSession': async () => {
    await subscriptionApi.startAuthSession('pixiv');
    await subscriptionApi.startAuthSession('mastodon', 'mastodon.social');
  },
  'subscriptionApi.cancelAuthSession': () => subscriptionApi.cancelAuthSession(),
  'subscriptionApi.getAuthSessionState': () => subscriptionApi.getAuthSessionState(),
  'subscriptionApi.saveManualOnlyFansCredential': () => subscriptionApi.saveManualOnlyFansCredential({
    cookie: 'sess=1', user_agent: 'Mozilla/5.0', x_bc: 'token',
  }),

  'tagApi.getTagsPaginated': async () => {
    await tagApi.getTagsPaginated({});
    await tagApi.getTagsPaginated({ namespace: 'character', search: 'ali', cursor: 'opaque', limit: 20 });
  },
  'tagApi.getNamespaceSummary': () => tagApi.getNamespaceSummary(),
  'tagApi.getTagsById': async () => {
    await tagApi.getTagsById([4, 5]);
    await tagApi.getTagsById([]);
  },
  'tagApi.getUnusedTagCount': () => tagApi.getUnusedTagCount(),
  'tagApi.renameTag': () => tagApi.renameTag(4, 'character:alice'),
  'tagApi.mergeTags': () => tagApi.mergeTags(5, 'cat'),
  'tagApi.deleteTag': () => tagApi.deleteTag(4),
  'tagApi.deleteUnusedTags': () => tagApi.deleteUnusedTags(),
  'tagApi.createTagGroup': () => tagApi.createTagGroup('species'),
  'tagApi.renameTagGroup': () => tagApi.renameTagGroup(2, 'creature'),
  'tagApi.deleteTagGroup': () => tagApi.deleteTagGroup(2),
  'taskApi.listTasks': () => taskApi.listTasks(),
  'taskApi.pauseTask': () => taskApi.pauseTask(1),
  'taskApi.resumeTask': () => taskApi.resumeTask(1),
  'taskApi.stopTask': async () => {
    await taskApi.stopTask(1, false);
    await taskApi.stopTask(1, true);
  },
  'taskApi.dismissTask': () => taskApi.dismissTask(1),
  'taskApi.taskFailures': () => taskApi.taskFailures(1),
};

/** Mirror the transport: Electron sends `JSON.stringify(args ?? {})` to Rust. */
function normalise(command: unknown, args: unknown): [string, unknown] {
  const json = JSON.stringify(args ?? {}, (_key, value) => {
    if (typeof value === 'bigint') {
      throw new Error(`${String(command)} sends a bigint, which the JSON transport cannot serialize`);
    }
    return value;
  });
  return [String(command), JSON.parse(json)];
}

describe('IPC argument contract', () => {
  it('preserves candidate order in the membership request and response', async () => {
    vi.mocked(invoke).mockReset().mockResolvedValueOnce([false, true, false]);

    await expect(entityApi.getRootMembership(filteredQuery, [2, 1, 2])).resolves.toEqual([false, true, false]);
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith('items.membership', {
      query: filteredQuery, root_ids: [2, 1, 2],
    });
  });

  it('covers every exported platform function', () => {
    const exported = Object.entries(MODULES).flatMap(([moduleName, module]) =>
      Object.entries(module)
        .filter(([, value]) => typeof value === 'function')
        .map(([name]) => `${moduleName as ModuleName}.${name}`));
    const listed = new Set([...Object.keys(CASES), ...Object.keys(NOT_IPC)]);
    expect(exported.filter((name) => !listed.has(name)).sort()).toEqual([]);
    expect([...listed].filter((name) => !exported.includes(name)).sort()).toEqual([]);
  });

  it('matches the recorded payload fixture the Rust dispatcher is tested against', async () => {
    const mockedInvoke = vi.mocked(invoke);
    mockedInvoke.mockReset();
    mockedInvoke.mockImplementation(async (command: string) => fakeResponse(command) as never);

    const silentCases = new Set([
      // Reads the resolved path, then needs the Electron search bridge.
      'shellApi.reverseImageSearch',
    ]);
    for (const [name, call] of Object.entries(CASES)) {
      const before = mockedInvoke.mock.calls.length;
      try {
        await call();
      } catch (error) {
        if (!silentCases.has(name)) throw new Error(`${name} failed: ${String(error)}`);
      }
      expect(mockedInvoke.mock.calls.length, `${name} never called invoke`).toBeGreaterThan(before);
    }

    const unique = new Map<string, [string, unknown]>();
    for (const [command, args] of mockedInvoke.mock.calls) {
      const entry = normalise(command, args);
      unique.set(JSON.stringify(entry), entry);
    }
    const calls = [...unique.entries()]
      .sort(([leftKey, [left]], [rightKey, [right]]) =>
        left < right ? -1 : left > right ? 1 : leftKey < rightKey ? -1 : leftKey > rightKey ? 1 : 0)
      .map(([, entry]) => entry);

    await expect(`${JSON.stringify(calls, null, 2)}\n`)
      .toMatchFileSnapshot('../../core/tests/fixtures/ipc_payloads.json');
  });
});
