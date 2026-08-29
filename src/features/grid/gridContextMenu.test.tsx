import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import type { MenuEntry, MenuItem } from '../../shared/ui/ContextMenu/ContextMenu';
import { buildEmptyContextMenu, buildEntityOpenContextEntries, buildExportContextEntry, buildTileContextMenu } from './gridContextMenu';

function submenuChildren(entries: MenuEntry[], label: string): MenuEntry[] {
  const entry = entries.find(
    (candidate) => 'submenu' in candidate && candidate.submenu && candidate.label === label,
  );
  if (!entry || !('children' in entry)) throw new Error(`missing ${label} submenu`);
  return entry.children;
}

describe('buildTileContextMenu', () => {
  it('finds every root containing the selected exact media for any media type', () => {
    const onFindMediaMatches = vi.fn();
    for (const singleMime of ['image/png', 'video/mp4', 'application/pdf']) {
      const entries = buildTileContextMenu({
        selectionCount: 1,
        querySelectionActive: false,
        singleSelected: true,
        singleHash: 'exact-hash',
        singleItemId: 42,
        singleKind: 'media',
        singleMime,
        scopeKind: 'system',
        statusFilter: null,
        loadedCount: 1,
        onFindMediaMatches,
      });
      const entry = submenuChildren(entries, 'More').find(
        (candidate): candidate is MenuItem => 'label' in candidate
          && candidate.label === 'Find Items with This Media',
      );
      expect(entry).toBeDefined();
      entry!.action();
    }
    expect(onFindMediaMatches).toHaveBeenCalledTimes(3);
    expect(onFindMediaMatches).toHaveBeenLastCalledWith(42);
  });

  it('offers every reverse-image provider for images and none for non-image media', () => {
    const onSearchByImage = vi.fn();
    const base = {
      selectionCount: 1,
      querySelectionActive: false,
      singleSelected: true,
      singleHash: 'image-hash',
      singleKind: 'media' as const,
      scopeKind: 'system' as const,
      statusFilter: null,
      loadedCount: 1,
      onSearchByImage,
    };
    const imageEntries = buildTileContextMenu({ ...base, singleMime: 'image/png' });
    const search = imageEntries.find(
      (entry) => 'submenu' in entry && entry.submenu && entry.label === 'Search by Image',
    );
    if (!search || !('children' in search)) throw new Error('missing reverse-image submenu');

    expect(search.children.map((entry) => 'label' in entry ? entry.label : '')).toEqual([
      'TinEye',
      'SauceNAO',
      'Yandex Images',
      'Sogou',
      'Bing Visual Search',
    ]);
    const sogou = search.children.find((entry) => 'label' in entry && entry.label === 'Sogou');
    if (!sogou || !('action' in sogou)) throw new Error('missing Sogou action');
    sogou.action();
    expect(onSearchByImage).toHaveBeenCalledWith('sogou', 'image-hash');

    const videoLabels = buildTileContextMenu({ ...base, singleMime: 'video/mp4' })
      .flatMap((entry) => ('label' in entry ? [entry.label] : []));
    expect(videoLabels).not.toContain('Search by Image');
  });

  it('shares one explicit export submenu across item and scope menus', () => {
    const originals = vi.fn();
    const converted = vi.fn();
    const entry = buildExportContextEntry({
      onExportOriginals: originals,
      onExportAs: converted,
    });

    if (!('children' in entry)) throw new Error('missing export submenu');
    expect(entry.children.map((child) => 'label' in child ? child.label : '')).toEqual([
      'Export Originals...',
      'Export As...',
    ]);
    if ('action' in entry.children[0]) entry.children[0].action();
    if ('action' in entry.children[1]) entry.children[1].action();
    expect(originals).toHaveBeenCalledOnce();
    expect(converted).toHaveBeenCalledOnce();
  });

  it('owns grayscale as a checked context command', () => {
    const onToggleGrayscale = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 1,
      querySelectionActive: false,
      singleSelected: true,
      singleHash: 'hash',
      singleKind: 'media',
      scopeKind: 'system',
      statusFilter: null,
      loadedCount: 1,
      grayscale: true,
      onToggleGrayscale,
    });
    const entry = entries.find(
      (candidate): candidate is MenuItem => 'label' in candidate && candidate.label === 'Grayscale',
    );

    expect(entry).toMatchObject({ checked: true, keepOpen: true, disabled: false });
    expect(renderToStaticMarkup(entry!.icon)).toContain('tabler-icon-contrast');
    entry!.action();
    expect(onToggleGrayscale).toHaveBeenCalledOnce();
  });

  it('exposes real creation and import actions on empty grid space', () => {
    const actions = {
      onNewFolder: vi.fn(),
      onNewSmartFolder: vi.fn(),
      onImportFiles: vi.fn(),
      onImportFolder: vi.fn(),
      onPasteImport: vi.fn(),
    };
    const entries = buildEmptyContextMenu({
      selectionCount: 0,
      querySelectionActive: false,
      singleSelected: false,
      singleHash: null,
      scopeKind: null,
      statusFilter: null,
      loadedCount: 0,
      ...actions,
    });
    const byLabel = new Map(entries.flatMap((entry) => (
      'label' in entry ? [[entry.label, entry] as const] : []
    )));

    expect([...byLabel.keys()]).toEqual(expect.arrayContaining([
      'New Folder', 'New Smart Folder', 'Import Files...', 'Import Folder...', 'Paste Import',
    ]));
    for (const [label, action] of [
      ['New Folder', actions.onNewFolder],
      ['New Smart Folder', actions.onNewSmartFolder],
      ['Import Files...', actions.onImportFiles],
      ['Import Folder...', actions.onImportFolder],
      ['Paste Import', actions.onPasteImport],
    ] as const) {
      const entry = byLabel.get(label);
      if (!entry || !('action' in entry)) throw new Error(`missing ${label}`);
      entry.action();
      expect(action).toHaveBeenCalledOnce();
    }
  });

  it('offers persisted content sorting on empty space inside a folder', () => {
    const onSortContents = vi.fn();
    const entries = buildEmptyContextMenu({
      selectionCount: 0,
      querySelectionActive: false,
      singleSelected: false,
      singleHash: null,
      scopeKind: 'folder',
      statusFilter: null,
      loadedCount: 0,
      onSortContents,
    });
    const size = submenuChildren(entries, 'Sort by').find(
      (entry): entry is MenuItem => 'label' in entry && entry.label === 'Size',
    );
    expect(size).toBeDefined();
    size!.action();
    expect(onSortContents).toHaveBeenCalledWith('size');
  });

  it('omits Paste Import when the clipboard has no importable payload', () => {
    const entries = buildEmptyContextMenu({
      selectionCount: 0,
      querySelectionActive: false,
      singleSelected: false,
      singleHash: null,
      scopeKind: null,
      statusFilter: null,
      loadedCount: 0,
      onImportFiles: vi.fn(),
      onImportFolder: vi.fn(),
    });
    const labels = entries.flatMap((entry) => ('label' in entry ? [entry.label] : []));
    expect(labels).not.toContain('Paste Import');
  });

  it('copies multi-item and collection selections through the selection target', () => {
    for (const context of [
      { selectionCount: 2, singleSelected: false, singleKind: null },
      { selectionCount: 1, singleSelected: true, singleKind: 'collection' as const },
    ]) {
      const onCopySelection = vi.fn();
      const onCopySelectionPaths = vi.fn();
      const onCopySelectionNames = vi.fn();
      const onCopySelectionLinks = vi.fn();
      const entries = buildTileContextMenu({
        ...context,
        querySelectionActive: false,
        singleHash: context.singleSelected ? 'cover-hash' : null,
        scopeKind: null,
        statusFilter: null,
        loadedCount: context.selectionCount,
        onCopySelection,
        onCopySelectionPaths,
        onCopySelectionNames,
        onCopySelectionLinks,
      });
      const actions = new Map(entries.flatMap((entry) => (
        'label' in entry && 'action' in entry ? [[entry.label, entry.action] as const] : []
      )));
      actions.get('Copy')?.();
      actions.get('Copy File Paths')?.();
      actions.get(context.selectionCount === 1 ? 'Copy Name' : 'Copy Names')?.();
      actions.get('Copy as Links')?.();
      expect(onCopySelection).toHaveBeenCalledOnce();
      expect(onCopySelectionPaths).toHaveBeenCalledOnce();
      expect(onCopySelectionNames).toHaveBeenCalledOnce();
      expect(onCopySelectionLinks).toHaveBeenCalledOnce();
    }
  });

  it('shows macOS Open With Other as an associated-application submenu', () => {
    const onOpenWithApplication = vi.fn();
    const entries = buildEntityOpenContextEntries({
      hash: 'hash',
      openWithOptions: {
        mode: 'submenu',
        applications: [{
          name: 'Preview',
          path: '/System/Applications/Preview.app',
          bundleIdentifier: 'com.apple.Preview',
          iconDataUrl: 'data:image/png;base64,AA==',
          isDefault: true,
        }],
      },
      onOpenWithApplication,
    });
    const submenu = entries.find(
      (entry) => 'submenu' in entry && entry.submenu && entry.label === 'Open With Other',
    );

    expect(submenu).toBeDefined();
    if (!submenu || !('children' in submenu)) throw new Error('missing submenu');
    expect(submenu.children[0]).toMatchObject({ label: 'Preview (Default)' });
    if ('action' in submenu.children[0]) submenu.children[0].action();
    expect(onOpenWithApplication).toHaveBeenCalledWith('hash', '/System/Applications/Preview.app');
  });

  it('reserves the Open With Other submenu while macOS discovers applications', () => {
    const entries = buildEntityOpenContextEntries({
      hash: 'hash',
      openWithOptions: null,
      openWithPending: true,
    });
    const submenu = entries.find(
      (entry) => 'submenu' in entry && entry.submenu && entry.label === 'Open With Other',
    );

    expect(submenu).toBeDefined();
    if (!submenu || !('children' in submenu)) throw new Error('missing pending submenu');
    expect(submenu.children[0]).toMatchObject({ label: 'Loading applications...', disabled: true });
  });

  it('uses the Windows system chooser for Open With Other', () => {
    const onOpenWithChooser = vi.fn();
    const entries = buildEntityOpenContextEntries({
      hash: 'hash',
      openWithOptions: { mode: 'chooser', applications: [] },
      onOpenWithChooser,
    });
    const entry = entries.find(
      (candidate): candidate is MenuItem => 'label' in candidate && candidate.label === 'Open With Other...',
    );

    expect(entry).toBeDefined();
    entry!.action();
    expect(onOpenWithChooser).toHaveBeenCalledWith('hash');
  });

  it('keeps Move to Trash non-destructive for a mixed selection', () => {
    const onMoveToTrash = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 2,
      querySelectionActive: false,
      singleSelected: false,
      singleHash: null,
      scopeKind: 'system',
      statusFilter: null,
      loadedCount: 2,
      isMixed: true,
      onMoveToTrash,
    });
    const trashEntry = entries.find(
      (entry): entry is MenuItem => 'label' in entry && entry.label === 'Move to Trash',
    );

    expect(trashEntry).toBeDefined();
    expect(trashEntry!.danger).toBeFalsy();
    trashEntry!.action();
    expect(onMoveToTrash).toHaveBeenCalledOnce();
  });

  it('accepts or rejects an Inbox multi-selection from the context menu', () => {
    const onAccept = vi.fn();
    const onReject = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 3,
      querySelectionActive: false,
      singleSelected: false,
      singleHash: null,
      scopeKind: 'system',
      statusFilter: 'inbox',
      loadedCount: 3,
      onAccept,
      onReject,
    });
    const actions = new Map(entries.flatMap((entry) => (
      'label' in entry && 'action' in entry ? [[entry.label, entry.action] as const] : []
    )));

    actions.get('Accept 3 Items')?.();
    actions.get('Reject 3 Items')?.();

    expect(onAccept).toHaveBeenCalledOnce();
    expect(onReject).toHaveBeenCalledOnce();
  });

  it('separates original export from converted export under one menu', () => {
    const onExport = vi.fn();
    const onExportOriginals = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 1,
      querySelectionActive: false,
      singleSelected: true,
      singleHash: 'hash',
      scopeKind: 'system',
      statusFilter: null,
      loadedCount: 1,
      onExport,
      onExportOriginals,
    });
    const exportEntry = entries.find(
      (entry) => 'label' in entry && entry.label === 'Export',
    );

    expect(exportEntry).toBeDefined();
    if (!exportEntry || !('children' in exportEntry)) throw new Error('missing export submenu');
    expect(renderToStaticMarkup(exportEntry.icon)).toContain('tabler-icon-file-export');
    expect(exportEntry.children.flatMap((entry) => 'label' in entry ? [entry.label] : [])).toEqual([
      'Export Originals...', 'Export As...',
    ]);
    if ('action' in exportEntry.children[0]) exportEntry.children[0].action();
    if ('action' in exportEntry.children[1]) exportEntry.children[1].action();
    expect(onExportOriginals).toHaveBeenCalledOnce();
    expect(onExport).toHaveBeenCalledOnce();
  });

  it('sets a single media item as the library cover', () => {
    const onSetLibraryCover = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 1,
      querySelectionActive: false,
      singleSelected: true,
      singleHash: 'image-hash',
      singleKind: 'media',
      singleMime: 'image/jpeg',
      scopeKind: 'system',
      statusFilter: null,
      loadedCount: 1,
      onSetLibraryCover,
    });
    const entry = submenuChildren(entries, 'More').find(
      (candidate): candidate is MenuItem => 'label' in candidate && candidate.label === 'Set as Library Cover',
    );

    expect(entry).toBeDefined();
    expect(renderToStaticMarkup(entry!.icon)).toContain('tabler-icon-photo');
    entry!.action();
    expect(onSetLibraryCover).toHaveBeenCalledWith('image-hash');
  });

  it('offers library cover for non-image media but not groups', () => {
    const base = {
      selectionCount: 1,
      querySelectionActive: false,
      singleSelected: true,
      singleHash: 'hash',
      scopeKind: 'system' as const,
      statusFilter: null,
      loadedCount: 1,
      onSetLibraryCover: vi.fn(),
    };
    const videoLabels = submenuChildren(
      buildTileContextMenu({ ...base, singleKind: 'media', singleMime: 'video/mp4' }),
      'More',
    )
      .flatMap((entry) => ('label' in entry ? [entry.label] : []));
    const groupLabels = buildTileContextMenu({ ...base, singleKind: 'collection', singleMime: 'image/jpeg', containsGroup: true })
      .flatMap((entry) => ('label' in entry ? [entry.label] : []));

    expect(videoLabels).toContain('Set as Library Cover');
    expect(groupLabels).not.toContain('Set as Library Cover');
  });

  it('disables auto tag when the current selection cannot be tagged', () => {
    const entries = buildTileContextMenu({
      selectionCount: 12,
      querySelectionActive: true,
      aiTagEnabled: false,
      singleSelected: false,
      singleHash: null,
      scopeKind: 'system',
      statusFilter: null,
      loadedCount: 50,
      onOpenAiTagger: vi.fn(),
    });
    const autoTagEntry = entries.find(
      (entry): entry is MenuItem => 'label' in entry && entry.label === 'Auto Tag 12 Images',
    );

    expect(autoTagEntry).toBeDefined();
    expect(autoTagEntry!.disabled).toBe(true);
  });

  it('uses Picto bookmark semantics for tag actions', () => {
    const entries = buildTileContextMenu({
      selectionCount: 1,
      querySelectionActive: false,
      singleSelected: true,
      singleHash: 'hash',
      hasClipboardTags: true,
      scopeKind: 'system',
      statusFilter: null,
      loadedCount: 1,
      onOpenTagSelect: vi.fn(),
      onOpenAiTagger: vi.fn(),
      onCopyTags: vi.fn(),
      onPasteTags: vi.fn(),
    });
    const byLabel = (label: string) => entries.find(
      (entry): entry is MenuItem => 'label' in entry && entry.label === label,
    );

    expect(renderToStaticMarkup(byLabel('Add Tags')!.icon)).toContain('tabler-icon-bookmark');
    expect(renderToStaticMarkup(byLabel('Auto Tag')!.icon)).toContain('data-icon="auto-tag"');
    expect(renderToStaticMarkup(byLabel('Copy Tags')!.icon)).toContain('tabler-icon-bookmarks');
    expect(renderToStaticMarkup(byLabel('Paste Tags')!.icon)).toContain('data-icon="paste-tags"');
  });

  it('adds the selection to the last used folder through a real handler', () => {
    const onAddToLastUsedFolder = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 3,
      querySelectionActive: false,
      singleSelected: false,
      singleHash: null,
      scopeKind: 'system',
      statusFilter: null,
      loadedCount: 3,
      lastUsedFolderName: 'References',
      onAddToLastUsedFolder,
    });
    const entry = entries.find(
      (candidate): candidate is MenuItem => 'label' in candidate && candidate.label === 'Add to “References”',
    );

    expect(entry).toBeDefined();
    expect(entry!.shortcut).toBe('Shift+D');
    entry!.action();
    expect(onAddToLastUsedFolder).toHaveBeenCalledOnce();
  });

  it('copies only shared tags for a multi-selection', () => {
    const onCopyTags = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 2,
      querySelectionActive: false,
      singleSelected: false,
      singleHash: null,
      hasClipboardTags: true,
      scopeKind: 'system',
      statusFilter: null,
      loadedCount: 2,
      onCopyTags,
      onPasteTags: vi.fn(),
    });
    const labels = entries.flatMap((entry) => ('label' in entry ? [entry.label] : []));

    expect(labels).not.toContain('Copy Tags');
    expect(labels).toContain('Copy Shared Tags');
    expect(labels).toContain('Paste Tags');
    const copyShared = entries.find(
      (entry): entry is MenuItem => 'label' in entry && entry.label === 'Copy Shared Tags',
    );
    copyShared!.action();
    expect(onCopyTags).toHaveBeenCalledOnce();
  });

  it('offers atomic batch rename for an explicit multi-selection', () => {
    const onBatchRename = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 3,
      querySelectionActive: false,
      singleSelected: false,
      singleHash: null,
      scopeKind: 'system',
      statusFilter: null,
      loadedCount: 3,
      onBatchRename,
    });
    const entry = entries.find(
      (candidate): candidate is MenuItem => 'label' in candidate && candidate.label === 'Batch Rename 3 Items...',
    );
    expect(entry).toBeDefined();
    entry!.action();
    expect(onBatchRename).toHaveBeenCalledOnce();
  });

  it('sets a selected member as the current folder cover', () => {
    const onSetFolderCover = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 1,
      querySelectionActive: false,
      singleSelected: true,
      singleHash: 'hash',
      scopeKind: 'folder',
      statusFilter: null,
      loadedCount: 1,
      onSetFolderCover,
    });
    const entry = submenuChildren(entries, 'More').find(
      (candidate): candidate is MenuItem => 'label' in candidate && candidate.label === 'Set as Folder Cover',
    );
    expect(entry).toBeDefined();
    entry!.action();
    expect(onSetFolderCover).toHaveBeenCalledOnce();
  });

  it('offers group organization for a multi-item selection', () => {
    const onOrganizeGroup = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 3,
      querySelectionActive: false,
      singleSelected: false,
      singleHash: null,
      scopeKind: 'system',
      statusFilter: 'active',
      loadedCount: 3,
      onOrganizeGroup,
    });
    const entry = entries.find(
      (candidate): candidate is MenuItem => 'label' in candidate && candidate.label === 'Group...',
    );

    expect(entry).toBeDefined();
    expect(renderToStaticMarkup(entry!.icon)).toContain('data-picto-icon="group-create"');
    entry!.action();
    expect(onOrganizeGroup).toHaveBeenCalledOnce();
  });

  it('treats a group as a library item rather than its cover file', () => {
    const onUngroup = vi.fn();
    const onOpenNewWindow = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 1,
      querySelectionActive: false,
      singleSelected: true,
      singleHash: 'cover-hash',
      singleKind: 'collection',
      containsGroup: true,
      scopeKind: 'system',
      statusFilter: 'active',
      loadedCount: 1,
      onOpen: vi.fn(),
      onOpenNewWindow,
      onEditGroup: vi.fn(),
      onUngroup,
      onOpenDefault: vi.fn(),
      onRevealInFolder: vi.fn(),
      onRegenerateThumbnails: vi.fn(),
    });
    const labels = entries.flatMap((entry) => ('label' in entry ? [entry.label] : []));

    expect(labels).not.toContain('Open');
    expect(labels).toContain('Open in New Window');
    expect(labels).toContain('Edit Group');
    expect(labels).toContain('Ungroup...');
    expect(labels).not.toContain('Open with Default App');
    expect(labels).not.toContain('Regenerate Thumbnail');
    expect(labels).not.toContain('Select All');
    expect(labels).not.toContain('Deselect All');

    const openWindow = entries.find(
      (entry): entry is MenuItem => 'label' in entry && entry.label === 'Open in New Window',
    );
    openWindow!.action();
    expect(onOpenNewWindow).toHaveBeenCalledOnce();

    const edit = entries.find(
      (entry): entry is MenuItem => 'label' in entry && entry.label === 'Edit Group',
    );
    expect(renderToStaticMarkup(edit!.icon)).toContain('data-picto-icon="group-edit"');

    const split = entries.find(
      (entry): entry is MenuItem => 'label' in entry && entry.label === 'Ungroup...',
    );
    expect(renderToStaticMarkup(split!.icon)).toContain('data-picto-icon="group-remove"');
    split!.action();
    expect(onUngroup).toHaveBeenCalledOnce();
  });
});
