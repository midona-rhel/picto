import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import type { MenuItem } from '../../shared/ui/ContextMenu/ContextMenu';
import { buildTileContextMenu } from './gridContextMenu';

describe('buildTileContextMenu', () => {
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

  it('uses the file-export icon for the export action', () => {
    const onExport = vi.fn();
    const entries = buildTileContextMenu({
      selectionCount: 1,
      querySelectionActive: false,
      singleSelected: false,
      singleHash: null,
      scopeKind: 'system',
      statusFilter: null,
      loadedCount: 1,
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
      onExport,
    });
    const exportEntry = entries.find(
      (entry): entry is MenuItem => 'label' in entry && entry.label === 'Export...',
    );

    expect(exportEntry).toBeDefined();
    expect(renderToStaticMarkup(exportEntry!.icon)).toContain('tabler-icon-file-export');
    exportEntry!.action();
    expect(onExport).toHaveBeenCalledOnce();
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
      singleSelected: false,
      singleHash: null,
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

  it('offers collection organization for a multi-item selection', () => {
    const onOrganizeCollection = vi.fn();
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
      onOrganizeCollection,
    });
    const entry = entries.find(
      (candidate): candidate is MenuItem => 'label' in candidate && candidate.label === 'Group into Collection...',
    );

    expect(entry).toBeDefined();
    entry!.action();
    expect(onOrganizeCollection).toHaveBeenCalledOnce();
  });

  it('treats a collection as a library item rather than its cover file', () => {
    const entries = buildTileContextMenu({
      selectionCount: 1,
      querySelectionActive: false,
      singleSelected: true,
      singleHash: 'cover-hash',
      singleKind: 'collection',
      containsCollection: true,
      scopeKind: 'system',
      statusFilter: 'active',
      loadedCount: 1,
      onSelectAll: vi.fn(),
      onDeselectAll: vi.fn(),
      onOpen: vi.fn(),
      onEditCollection: vi.fn(),
      onOpenDefault: vi.fn(),
      onRevealInFolder: vi.fn(),
      onRegenerateThumbnails: vi.fn(),
    });
    const labels = entries.flatMap((entry) => ('label' in entry ? [entry.label] : []));

    expect(labels).toContain('Open');
    expect(labels).toContain('Edit Collection');
    expect(labels).not.toContain('Open with Default App');
    expect(labels).not.toContain('Regenerate Thumbnail');
  });
});
