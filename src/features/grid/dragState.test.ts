import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  cancelDrag,
  getDragState,
  moveDrag,
  resolveDropTarget,
  startDrag,
} from './dragState';

describe('grid drag target resolution', () => {
  const elementFromPoint = vi.fn<[], Element | null>();

  beforeEach(() => {
    document.body.replaceChildren();
    elementFromPoint.mockReset();
    Object.defineProperty(document, 'elementFromPoint', {
      configurable: true,
      value: elementFromPoint,
    });
  });

  afterEach(() => cancelDrag());

  it('resolves nested content to its exact folder drop surface', () => {
    const row = document.createElement('div');
    row.dataset.folderDropId = '42';
    const icon = document.createElement('span');
    row.append(icon);

    expect(resolveDropTarget(icon)).toEqual({
      target: { kind: 'folder', folderId: 42, nodeId: 'folder:42' },
      element: row,
    });
  });

  it('moves the highlight between duplicate views of the same folder', () => {
    const pinnedRow = document.createElement('div');
    pinnedRow.dataset.folderDropId = '42';
    const treeRow = document.createElement('div');
    treeRow.dataset.folderDropId = '42';
    document.body.append(pinnedRow, treeRow);
    elementFromPoint.mockReturnValueOnce(pinnedRow).mockReturnValueOnce(treeRow);

    startDrag(['hash'], 0, 0, { kind: 'all' });
    moveDrag(10, 10);
    expect(pinnedRow.dataset.dropHighlighted).toBe('true');
    expect(treeRow.dataset.dropHighlighted).toBeUndefined();

    moveDrag(10, 20);
    expect(pinnedRow.dataset.dropHighlighted).toBeUndefined();
    expect(treeRow.dataset.dropHighlighted).toBe('true');
    expect(getDragState().dropTarget).toEqual({ kind: 'folder', folderId: 42, nodeId: 'folder:42' });
  });

  it('clears only the owned highlight when leaving a target', () => {
    const row = document.createElement('div');
    row.dataset.statusDrop = '2';
    const unrelated = document.createElement('div');
    unrelated.dataset.dropHighlighted = 'true';
    document.body.append(row, unrelated);
    elementFromPoint.mockReturnValueOnce(row).mockReturnValueOnce(null);

    startDrag(['hash'], 0, 0, { kind: 'all' });
    moveDrag(10, 10);
    moveDrag(20, 20);

    expect(row.dataset.dropHighlighted).toBeUndefined();
    expect(unrelated.dataset.dropHighlighted).toBe('true');
    expect(getDragState().dropTarget).toBeNull();
  });
});
