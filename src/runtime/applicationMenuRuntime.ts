import { getDefaultStore } from 'jotai';
import { appController } from '../controllers/appController';
import * as entityMutations from '../controllers/entityMutations';
import { chooseAndImportFiles, chooseAndImportFolder, filesController } from '../controllers/filesController';
import { foldersController } from '../controllers/foldersController';
import { windowController } from '../controllers/windowController';
import { listen } from '../platform/ipc';
import { gridActiveAtom, gridItemsAtom, gridScopeAtom } from '../state/grid';
import {
  batchRenameModalAtom,
  confirmModalAtom,
  exportModalAtom,
  folderPickerModalAtom,
  smartFolderModalAtom,
  tagSelectModalAtom,
  updateModalAtom,
} from '../state/modals';
import { aiTaggerPortalAtom, inspectorAnchor } from '../state/portals';
import {
  clearSelectionAtom,
  gridSelectionAtom,
  selectionCountAtom,
  selectionTargetAtom,
} from '../state/selection';
import type { CanonicalEntityGridItem, EntityTarget } from '../shared/types/canonical';
import { showErrorNotification, showInfoNotification, showSuccessNotification } from '../shared/lib/notifications';

type ApplicationMenuEvent =
  | 'menu:import-files'
  | 'menu:import-folder'
  | 'menu:export-basic'
  | 'menu:export-advanced'
  | 'menu:show-updates';

type SelectionMenuAction =
  | 'new-folder'
  | 'new-smart-folder'
  | 'rename'
  | 'copy-paths'
  | 'copy-names'
  | 'copy-links'
  | 'regenerate-thumbnails'
  | 'deselect-all'
  | 'remove-from-folder'
  | 'accept'
  | 'reject'
  | 'move-to-trash'
  | 'restore'
  | 'delete-permanently'
  | 'add-tags'
  | 'add-to-folder'
  | 'auto-tag'
  | 'set-rating'
  | 'open-default'
  | 'reveal-in-folder'
  | 'open-new-window';

interface SelectionMenuPayload {
  action: SelectionMenuAction;
  rating?: number;
}

const store = getDefaultStore();

function selectionSnapshot() {
  const count = store.get(selectionCountAtom);
  const target = store.get(selectionTargetAtom);
  const selection = store.get(gridSelectionAtom);
  const items = store.get(gridItemsAtom);
  const selected = selection.mode === 'explicit'
    ? items.filter((item) => selection.itemIds.has(item.root_id))
    : [];
  return { count, target, selection, items, selected };
}

function menuContext(): Record<string, unknown> {
  const { count, selection, selected } = selectionSnapshot();
  const gridActive = store.get(gridActiveAtom);
  const contextualCount = gridActive ? count : 0;
  const scope = store.get(gridScopeAtom);
  const single = selection.mode === 'explicit' && count === 1 ? selected[0] ?? null : null;
  const commonLifecycle = selected.length > 0 && selected.every((item) => item.lifecycle === selected[0]?.lifecycle)
    ? selected[0]?.lifecycle ?? null
    : null;
  const statusFilter = commonLifecycle === 'trash' || scope.kind === 'trash' ? 'trash'
    : commonLifecycle === 'inbox' || scope.kind === 'inbox' ? 'inbox'
    : commonLifecycle;
  return {
    selectionCount: contextualCount,
    singleSelected: gridActive && Boolean(single),
    singleKind: gridActive ? single?.kind ?? null : null,
    scopeKind: scope.kind,
    statusFilter,
    canRename: gridActive && selection.mode === 'explicit' && selected.length === count && count > 0,
    canCopyNames: gridActive && selection.mode === 'explicit' && selected.length === count && count > 0,
    canRegenerateThumbnails: gridActive && selection.mode === 'explicit'
      && selected.length === count
      && selected.length > 0
      && selected.every((item) => item.kind === 'media' && Boolean(item.content_hash)),
  };
}

function selectedExport() {
  return {
    count: store.get(selectionCountAtom),
    target: store.get(selectionTargetAtom),
  };
}

async function exportOriginals(): Promise<void> {
  const { count, target } = selectedExport();
  if (!target || count === 0) {
    showInfoNotification({ title: 'Select files to export', message: '' });
    return;
  }
  const result = await (window as any).picto.dialog.open({
    properties: ['openDirectory'],
    multiple: false,
    title: 'Choose export folder',
  });
  if (!result) return;
  const outputDir = typeof result === 'string' ? result : result[0];
  if (!outputDir) return;
  await filesController.exportMedia(target, { output_dir: outputDir, format: 'original' });
  showSuccessNotification({
    title: `Exported ${count} file${count === 1 ? '' : 's'}`,
    message: '',
  });
}

async function runMenuAction(name: ApplicationMenuEvent): Promise<void> {
  if (name === 'menu:show-updates') {
    store.set(updateModalAtom, { open: true });
    return;
  }
  if (name === 'menu:import-files') {
    await chooseAndImportFiles(store.get(gridScopeAtom));
    return;
  }
  if (name === 'menu:import-folder') {
    await chooseAndImportFolder(store.get(gridScopeAtom));
    return;
  }
  if (name === 'menu:export-basic') {
    await exportOriginals();
    return;
  }
  const { count, target } = selectedExport();
  if (!target || count === 0) {
    showInfoNotification({ title: 'Select files to export', message: '' });
    return;
  }
  store.set(exportModalAtom, { open: true, fileCount: count, target });
}

function requireTarget(): EntityTarget {
  const target = store.get(selectionTargetAtom);
  if (!target) throw new Error('Select one or more items first.');
  return target;
}

function singleSelectedItem(): CanonicalEntityGridItem {
  const { count, selected } = selectionSnapshot();
  if (count !== 1 || selected.length !== 1) throw new Error('Select exactly one item first.');
  return selected[0];
}

async function runSelectionAction({ action, rating }: SelectionMenuPayload): Promise<void> {
  const snapshot = selectionSnapshot();
  const scope = store.get(gridScopeAtom);
  if (action === 'new-folder') {
    const parentId = scope.kind === 'folder' ? scope.folder_id : null;
    await foldersController.create('New Folder', parentId);
    return;
  }
  if (action === 'new-smart-folder') {
    store.set(smartFolderModalAtom, { open: true, mode: 'create', editor: 'all' });
    return;
  }
  if (action === 'deselect-all') {
    store.set(clearSelectionAtom);
    return;
  }
  if (action === 'add-tags') {
    requireTarget();
    store.set(tagSelectModalAtom, { open: true });
    return;
  }
  if (action === 'add-to-folder') {
    requireTarget();
    store.set(folderPickerModalAtom, { open: true });
    return;
  }
  if (action === 'auto-tag') {
    const target = requireTarget();
    store.set(aiTaggerPortalAtom, { open: true, target, anchor: inspectorAnchor() });
    return;
  }
  if (action === 'set-rating') {
    await entityMutations.setTargetRating(requireTarget(), rating ?? 0);
    return;
  }
  if (action === 'move-to-trash') {
    await entityMutations.setTargetLifecycle(requireTarget(), 'trash');
    return;
  }
  if (action === 'accept') {
    await entityMutations.setTargetLifecycle(requireTarget(), 'active');
    return;
  }
  if (action === 'reject') {
    await entityMutations.setTargetLifecycle(requireTarget(), 'trash');
    return;
  }
  if (action === 'restore') {
    await entityMutations.setTargetLifecycle(requireTarget(), 'active');
    return;
  }
  if (action === 'delete-permanently') {
    const target = requireTarget();
    const count = snapshot.count;
    store.set(confirmModalAtom, {
      open: true,
      title: count === 1 ? 'Delete permanently?' : `Delete ${count} items permanently?`,
      message: 'This removes the files from the library and cannot be undone.',
      confirmLabel: 'Delete Permanently',
      danger: true,
      onConfirm: () => { void entityMutations.permanentlyDeleteTarget(target).catch(reportMenuError); },
    });
    return;
  }
  if (action === 'remove-from-folder') {
    if (scope.kind !== 'folder') throw new Error('Open a folder before removing items from it.');
    await entityMutations.updateTargetFolderMembership(requireTarget(), scope.folder_id, 'remove');
    entityMutations.settleSelectionAfterMutation();
    return;
  }
  if (action === 'copy-paths') {
    await filesController.copyTargetPaths(requireTarget());
    return;
  }
  if (action === 'copy-links') {
    await filesController.copyTargetLinks(requireTarget());
    return;
  }
  if (action === 'copy-names') {
    if (snapshot.selected.length !== snapshot.count || snapshot.count === 0) throw new Error('Names are unavailable for this selection.');
    filesController.copyText(snapshot.selected.map((item) => item.name || 'Untitled').join('\n'));
    return;
  }
  if (action === 'rename') {
    if (snapshot.selected.length !== snapshot.count || snapshot.count === 0) throw new Error('Rename requires a loaded selection.');
    store.set(batchRenameModalAtom, {
      open: true,
      items: snapshot.selected.map((item) => ({ root_id: item.root_id, name: item.name || 'Untitled' })),
    });
    return;
  }
  if (action === 'regenerate-thumbnails') {
    const hashes = snapshot.selected.filter((item) => item.kind === 'media').map((item) => item.content_hash).filter(Boolean);
    if (hashes.length !== snapshot.count) throw new Error('Thumbnails are unavailable for this selection.');
    await filesController.regenerateThumbnailsBatch(hashes);
    return;
  }

  const item = singleSelectedItem();
  if (action === 'open-new-window') {
    if (item.kind === 'collection') await windowController.openDetailWindow({ item_id: item.root_id });
    else await windowController.openDetailWindow({ hash: item.content_hash, width: item.width, height: item.height });
    return;
  }
  if (item.kind !== 'media' || !item.content_hash) throw new Error('This action requires one media item.');
  if (action === 'open-default') await filesController.openDefaultAppForHash(item.content_hash);
  else if (action === 'reveal-in-folder') await filesController.revealHashInFolder(item.content_hash);
}

function reportMenuError(error: unknown): void {
  showErrorNotification({
    title: 'Menu action failed',
    message: error instanceof Error ? error.message : String(error),
  });
}

export function startApplicationMenuRuntime(): () => void {
  let disposed = false;
  let contextQueued = false;
  let lastContext = '';
  const disposers: Array<() => void> = [];
  const syncContext = () => {
    if (disposed || contextQueued) return;
    contextQueued = true;
    queueMicrotask(() => {
      contextQueued = false;
      if (disposed) return;
      const context = menuContext();
      const fingerprint = JSON.stringify(context);
      if (fingerprint === lastContext) return;
      lastContext = fingerprint;
      void appController.syncApplicationMenuContext(context).catch((error) => {
        console.error('Failed to synchronize application menu context', error);
      });
    });
  };

  const names: ApplicationMenuEvent[] = [
    'menu:import-files',
    'menu:import-folder',
    'menu:export-basic',
    'menu:export-advanced',
    'menu:show-updates',
  ];
  for (const name of names) {
    void listen(name, () => { void runMenuAction(name).catch(reportMenuError); }).then((dispose) => {
      if (disposed) dispose();
      else disposers.push(dispose);
    }).catch((error) => {
      console.error(`Failed to subscribe to ${name}`, error);
    });
  }
  void listen<SelectionMenuPayload>('menu:selection-action', ({ payload }) => {
    void runSelectionAction(payload).catch(reportMenuError);
  }).then((dispose) => {
    if (disposed) dispose();
    else disposers.push(dispose);
  }).catch((error) => {
    console.error('Failed to subscribe to selection menu actions', error);
  });

  disposers.push(store.sub(gridSelectionAtom, syncContext));
  disposers.push(store.sub(gridItemsAtom, syncContext));
  disposers.push(store.sub(gridScopeAtom, syncContext));
  disposers.push(store.sub(gridActiveAtom, syncContext));
  syncContext();

  return () => {
    disposed = true;
    for (const dispose of disposers) dispose();
  };
}
