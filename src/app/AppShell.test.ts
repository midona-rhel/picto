import { describe, expect, it, vi } from 'vitest';
import { isEditableTarget } from './editableTarget';
import { buildPanelVisibilityContextEntries, ScopeTitle } from './AppShell';
import { createElement } from 'react';
import { render } from '@testing-library/react';
import { createStore, Provider } from 'jotai';
import { displayedSurfaceNodeIdAtom } from '../state/navigation';
import { displayedGridSnapshotAtom, displayedInspectorTargetAtom, scopeInspectorViewModelAtom } from '../state/inspector';
import { gridSessionAtom } from '../state/grid';
import { sidebarNodesAtom } from '../state/sidebar';
import { createEmptyItemFilters } from '../shared/lib/itemFilters';

describe('scope title', () => {
  it('does not carry grid search results into the tag manager', () => {
    const store = createStore();
    store.set(displayedSurfaceNodeIdAtom, 'system:tag_manager');
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: false });
    store.set(displayedGridSnapshotAtom, {
      nodeId: 'system:active', previewItems: [], totalCount: 9,
      totalSizeBytes: 0, searchText: 'previous search',
      filters: createEmptyItemFilters(), sidebarNode: null,
    });
    const { container, unmount } = render(createElement(Provider, { store }, createElement(ScopeTitle)));
    expect(container.textContent).toBe('Tags');
    unmount();
  });

  it('keeps tag drill-down results and inspector counts tied to the displayed grid', () => {
    const store = createStore();
    store.set(displayedSurfaceNodeIdAtom, 'system:tag_manager');
    store.set(displayedInspectorTargetAtom, { kind: 'scope', nodeId: 'system:tag_manager' });
    store.set(sidebarNodesAtom, [
      { id: 'system:active', name: 'All', kind: 'system', parent_id: null, count: 1000000, sort_order: 0, freshness: 'fresh', selectable: true },
      { id: 'system:tag_manager', name: 'Tags', kind: 'system', parent_id: null, count: 6, sort_order: 1, freshness: 'fresh', selectable: true },
    ]);
    store.set(displayedGridSnapshotAtom, {
      nodeId: 'system:active', previewItems: [], totalCount: 10001,
      totalSizeBytes: 2000, searchText: 'landscape',
      filters: createEmptyItemFilters(), sidebarNode: null,
    });
    const { container, unmount } = render(createElement(Provider, { store }, createElement(ScopeTitle)));
    expect(container.textContent).toContain('Tags');
    expect(container.textContent).toContain('Search results');
    expect(store.get(scopeInspectorViewModelAtom)).toMatchObject({ totalCount: 10001, totalSizeBytes: 2000 });
    store.set(sidebarNodesAtom, [...store.get(sidebarNodesAtom), {
      id: 'folder:7', name: 'Child', kind: 'folder', parent_id: null,
      count: 3, freshness: 'fresh', selectable: true,
    }]);
    store.set(displayedInspectorTargetAtom, { kind: 'scope', nodeId: 'folder:7' });
    expect(store.get(scopeInspectorViewModelAtom)).toMatchObject({ totalCount: 3 });
    unmount();
  });
});

describe('isEditableTarget', () => {
  it('recognizes editable controls and contenteditable elements', () => {
    expect(isEditableTarget(document.createElement('input'))).toBe(true);
    expect(isEditableTarget(document.createElement('textarea'))).toBe(true);
    expect(isEditableTarget(document.createElement('select'))).toBe(true);

    const editable = document.createElement('div');
    editable.setAttribute('contenteditable', 'true');
    expect(isEditableTarget(editable)).toBe(true);

    const child = document.createElement('span');
    editable.append(child);
    expect(isEditableTarget(child)).toBe(true);
  });

  it('leaves non-editable targets eligible for app shortcuts', () => {
    expect(isEditableTarget(document.createElement('button'))).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
  });
});

describe('panel visibility context menu', () => {
  it('uses shared panel actions and keeps the menu open for successive toggles', () => {
    const toggleAll = vi.fn();
    const toggleSidebar = vi.fn();
    const toggleInspector = vi.fn();
    const entries = buildPanelVisibilityContextEntries({ toggleAll, toggleSidebar, toggleInspector });

    expect(entries.map((entry) => 'label' in entry ? entry.label : null)).toEqual([
      'Toggle All Panels',
      'Toggle Sidebar',
      'Toggle Inspector',
    ]);
    for (const entry of entries) {
      expect('keepOpen' in entry && entry.keepOpen).toBe(true);
    }
    if ('action' in entries[1]) entries[1].action();
    expect(toggleSidebar).toHaveBeenCalledOnce();
  });
});
