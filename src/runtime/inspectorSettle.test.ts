import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import {
  displayedGridSnapshotAtom,
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorPinnedAtom,
} from '../state/inspector';
import { gridSessionAtom } from '../state/grid';
import { activeNodeIdAtom } from '../state/navigation';
import { selectedSubfolderNodeIdsAtom } from '../state/selection';

let eventHandler: ((event: { payload: { changes: Record<string, unknown> } }) => void) | undefined;

vi.mock('../platform/ipc', () => ({
  listen: vi.fn(async (
    _name: string,
    handler: (event: { payload: { changes: Record<string, unknown> } }) => void,
  ) => {
    eventHandler = handler;
    return () => {};
  }),
}));

const inspectorControllerMocks = vi.hoisted(() => ({
  commitInspectorTarget: vi.fn(),
  loadInspectorData: vi.fn(),
  loadSubfolderInspectorPreview: vi.fn(),
}));
vi.mock('../controllers/inspectorController', () => inspectorControllerMocks);

import { startInspectorSettle } from './inspectorSettle';

const store = getDefaultStore();

describe('inspector runtime settling', () => {
  afterEach(() => {
    eventHandler = undefined;
    inspectorControllerMocks.commitInspectorTarget.mockReset();
    inspectorControllerMocks.loadInspectorData.mockReset();
    inspectorControllerMocks.loadSubfolderInspectorPreview.mockReset();
    store.set(displayedInspectorEntityDataAtom, null);
    store.set(displayedInspectorTargetAtom, { kind: 'none' });
    store.set(displayedGridSnapshotAtom, null);
    store.set(selectedSubfolderNodeIdsAtom, new Set());
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true });
    store.set(inspectorPinnedAtom, false);
  });

  it('reloads the displayed entity after tag structure changes', async () => {
    store.set(displayedInspectorEntityDataAtom, { entity_hash: 'entity-1' } as never);
    const stop = startInspectorSettle();
    await Promise.resolve();

    eventHandler?.({
      payload: {
        changes: {
          tag_structure_changed: true,
          entity_hashes: ['another-entity'],
        },
      },
    });

    expect(inspectorControllerMocks.loadInspectorData).toHaveBeenCalledWith('entity-1');
    stop();
  });

  it('delegates selected subfolder previews to inspector ownership', () => {
    store.set(activeNodeIdAtom, 'folder:1');
    const stop = startInspectorSettle();

    store.set(selectedSubfolderNodeIdsAtom, new Set(['folder:2']));

    expect(inspectorControllerMocks.loadSubfolderInspectorPreview).toHaveBeenCalledWith('folder:2');
    stop();
  });

  it('restores the displayed parent scope when subfolder selection clears', () => {
    store.set(activeNodeIdAtom, 'folder:1');
    store.set(displayedGridSnapshotAtom, {
      nodeId: 'folder:1',
      previewItems: [],
      totalCount: 0,
      totalSizeBytes: 0,
      searchText: '',
      sidebarNode: null,
    });
    store.set(displayedInspectorTargetAtom, { kind: 'scope', nodeId: 'folder:2' });
    store.set(selectedSubfolderNodeIdsAtom, new Set(['folder:2']));
    const stop = startInspectorSettle();

    store.set(selectedSubfolderNodeIdsAtom, new Set());

    expect(inspectorControllerMocks.commitInspectorTarget).toHaveBeenCalledWith({
      kind: 'scope',
      nodeId: 'folder:1',
    });
    stop();
  });
});
