import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CanonicalEntityGridItem, EntityViewPage } from '../shared/types/canonical';
import { gridSessionAtom } from '../state/grid';
import {
  displayedInspectorTargetAtom,
  subfolderPreviewAtom,
} from '../state/inspector';
import { activeNodeIdAtom } from '../state/navigation';
import { emptyGridSelection, gridSelectionAtom } from '../state/selection';

const entityApiMocks = vi.hoisted(() => ({
  getEntityDetails: vi.fn(),
  queryEntityView: vi.fn(),
}));
vi.mock('../platform/entityApi', () => entityApiMocks);

import { loadSubfolderInspectorPreview } from './inspectorController';

const store = getDefaultStore();

function item(hash: string): CanonicalEntityGridItem {
  return {
    entity_id: 2,
    entity_hash: hash,
    name: 'Preview',
    mime_type: 'image/jpeg',
    pixel_width: 100,
    pixel_height: 100,
    status: 1,
    rating: null,
    date_added: '2026-01-01',
    date_created: '2026-01-01',
    date_modified: '2026-01-01',
    has_thumbnail: true,
    duration_ms: null,
    frame_count: null,
    has_audio: false,
    dominant_color_hex: null,
    size_bytes: 10,
  };
}

function page(): EntityViewPage {
  return {
    items: [item('preview-1')],
    next_cursor: null,
    total_count: 1,
    total_size_bytes: 10,
  };
}

describe('subfolder inspector preview', () => {
  beforeEach(() => {
    entityApiMocks.queryEntityView.mockReset();
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true });
    store.set(activeNodeIdAtom, 'folder:1');
    store.set(gridSelectionAtom, { ...emptyGridSelection(), folderNodeIds: new Set(['folder:2']) });
    store.set(displayedInspectorTargetAtom, { kind: 'scope', nodeId: 'folder:1' });
    store.set(subfolderPreviewAtom, null);
  });

  it('commits preview data and its target together', async () => {
    entityApiMocks.queryEntityView.mockResolvedValueOnce(page());

    await loadSubfolderInspectorPreview('folder:2');

    expect(entityApiMocks.queryEntityView).toHaveBeenCalledWith({
      base_scope: { kind: 'folder', id: 2 },
      page: { limit: 4 },
    });
    expect(store.get(displayedInspectorTargetAtom)).toEqual({ kind: 'scope', nodeId: 'folder:2' });
    expect(store.get(subfolderPreviewAtom)?.items[0].entity_hash).toBe('preview-1');
  });

  it('does not commit a preview after that subfolder is deselected', async () => {
    let resolvePage: ((result: EntityViewPage) => void) | undefined;
    entityApiMocks.queryEntityView.mockImplementationOnce(
      () => new Promise<EntityViewPage>((resolve) => { resolvePage = resolve; }),
    );

    const loading = loadSubfolderInspectorPreview('folder:2');
    store.set(gridSelectionAtom, emptyGridSelection());
    resolvePage?.(page());
    await loading;

    expect(store.get(displayedInspectorTargetAtom)).toEqual({ kind: 'scope', nodeId: 'folder:1' });
    expect(store.get(subfolderPreviewAtom)).toBeNull();
  });
});
