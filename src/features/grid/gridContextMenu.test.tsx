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
});
