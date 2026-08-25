import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import type { MenuItem } from '../../shared/ui/ContextMenu/ContextMenu';
import { buildEmptyContextMenu, buildEntityOpenContextEntries, buildExportContextEntry, buildTileContextMenu } from './gridContextMenu';

describe('buildTileContextMenu', () => {
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
    });
    const entry = entries.find(
      (candidate): candidate is MenuItem => 'label' in candidate && candidate.label === 'Grayscale',
    );

    expect(entry).toMatchObject({ checked: true, keepOpen: true, disabled: false });
    expect(renderToStaticMarkup(entry!.icon)).toContain('tabler-icon-contrast-filled');
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
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

  it('omits Paste Import when the clipboard has no importable payload', () => {
    const entries = buildEmptyContextMenu({
      selectionCount: 0,
      querySelectionActive: false,
      singleSelected: false,
      singleHash: null,
      scopeKind: null,
      statusFilter: null,
      loadedCount: 0,
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
      onImportFiles: vi.fn(),
      onImportFolder: vi.fn(),
    });
    const labels = entries.flatMap((entry) => ('label' in entry ? [entry.label] : []));
    expect(labels).not.toContain('Paste Import');
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
      onSetLibraryCover,
    });
    const entry = entries.find(
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
      onSetLibraryCover: vi.fn(),
    };
    const videoLabels = buildTileContextMenu({ ...base, singleKind: 'media', singleMime: 'video/mp4' })
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
      onAddToLastUsedFolder,
    });
    const entry = entries.find(
      (candidate): candidate is MenuItem => 'label' in candidate && candidate.label === 'Add to Last Used Folder',
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
      onSetFolderCover,
    });
    const entry = entries.find(
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
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
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
      onOpen: vi.fn(),
      onEditGroup: vi.fn(),
      onUngroup,
      onOpenDefault: vi.fn(),
      onRevealInFolder: vi.fn(),
      onRegenerateThumbnails: vi.fn(),
    });
    const labels = entries.flatMap((entry) => ('label' in entry ? [entry.label] : []));

    expect(labels).toContain('Open');
    expect(labels).toContain('Edit Group');
    expect(labels).toContain('Ungroup...');
    expect(labels).not.toContain('Open with Default App');
    expect(labels).not.toContain('Regenerate Thumbnail');

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
