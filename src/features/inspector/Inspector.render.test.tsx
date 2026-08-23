import { fireEvent, render, screen } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { MantineProvider } from '@mantine/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const useAtomValue = vi.hoisted(() => vi.fn());

vi.mock('jotai', async (importOriginal) => ({
  ...(await importOriginal<typeof import('jotai')>()),
  useAtomValue,
}));

vi.mock('../../controllers/entityMutations', () => ({
  getTargetSelectionSummary: vi.fn(),
  removeEntityFromFolder: vi.fn(),
  removeEntityTags: vi.fn(),
  removeTargetTags: vi.fn(),
  setEntityName: vi.fn(),
  setEntityNotes: vi.fn(),
  setEntityRating: vi.fn(),
  setEntitySourceUrls: vi.fn(),
  setTargetNotes: vi.fn(),
  setTargetRating: vi.fn(),
  updateTargetFolderMembership: vi.fn(),
}));

import { Inspector } from './Inspector';
import {
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  scopeInspectorViewModelAtom,
} from '../../state/inspector';
import { sidebarNodesAtom } from '../../state/sidebar';
import { gridItemsAtom } from '../../state/grid';
import {
  selectionCountAtom,
  selectionFingerprintAtom,
  selectionTargetAtom,
  selectedEntityHashesAtom,
} from '../../state/selection';
import { folderPickerPortalAtom, tagSelectPortalAtom } from '../../state/portals';

const entity = {
  entity_hash: 'entity-1', name: 'Example', mime_type: 'image/jpeg', size_bytes: 100,
  pixel_width: 20, pixel_height: 10, duration_ms: null, frame_count: null, has_audio: false,
  status: 0, rating: null, notes: null, source_urls: [], date_created: '2026-01-01',
  date_added: '2026-01-02', date_modified: '2026-01-03', dominant_color_hex: null,
  dominant_colors: [], perceptual_hash: null, tags: [], folders: [],
};

function renderInspector({ target, data = null, scope = null, loading = false, error = null }: {
  target: unknown; data?: unknown; scope?: unknown; loading?: boolean; error?: string | null;
}) {
  const values = new Map<unknown, unknown>([
    [displayedInspectorTargetAtom, target],
    [inspectorLoadingAtom, loading],
    [inspectorErrorAtom, error],
    [displayedInspectorEntityDataAtom, data],
    [scopeInspectorViewModelAtom, scope],
    [sidebarNodesAtom, []],
    [gridItemsAtom, []],
    [selectionTargetAtom, null],
    [selectedEntityHashesAtom, new Set()],
    [selectionCountAtom, target && (target as { kind?: string }).kind === 'multi' ? 2 : 0],
    [selectionFingerprintAtom, 'test'],
  ]);
  useAtomValue.mockImplementation((atom) => values.get(atom));
  return render(<MantineProvider><Inspector /></MantineProvider>);
}

function assertStableAnchors(expectedCoreLabels: string[]) {
  expect([...document.querySelectorAll('[data-inspector-anchor]')].map((node) => node.getAttribute('data-inspector-anchor')))
    .toEqual(['name', 'notes', 'source']);
  expect([...document.querySelectorAll('[data-inspector-core-property]')].map((node) => node.getAttribute('data-inspector-core-property')))
    .toEqual(expectedCoreLabels);
}

function assertNoClassificationSections() {
  expect(document.querySelector('[data-inspector-section="tags"]')).not.toBeInTheDocument();
  expect(document.querySelector('[data-inspector-section="folders"]')).not.toBeInTheDocument();
  expect(document.querySelector('[data-inspector-empty-action]')).not.toBeInTheDocument();
}

describe('Inspector presentation branches', () => {
  beforeEach(() => {
    useAtomValue.mockReset();
    const store = getDefaultStore();
    store.set(tagSelectPortalAtom, { open: false, anchor: null });
    store.set(folderPickerPortalAtom, { open: false, anchor: null });
  });

  it('keeps identity and core anchors for entity, multi, loading, and error', () => {
    const cases = [
      { target: { kind: 'entity', entityHash: 'entity-1' }, data: entity, unavailableSource: false },
      { target: { kind: 'multi', count: 2, selectionMode: 'explicit' }, data: null, unavailableSource: true },
      { target: { kind: 'entity', entityHash: 'entity-1' }, data: null, loading: true, unavailableSource: true },
      { target: { kind: 'entity', entityHash: 'entity-1' }, data: null, error: 'Unavailable', unavailableSource: true },
    ];

    for (const entry of cases) {
      const view = renderInspector(entry);
      assertStableAnchors(entry.data
        ? ['Items', 'Dimensions', 'Size', 'Type', 'Date added', 'Date created', 'Date modified']
        : entry.target.kind === 'multi' ? ['Items'] : []);
      const sections = [...document.querySelectorAll('[data-inspector-section]')].map((node) => node.getAttribute('data-inspector-section'));
      if (entry.data || entry.target.kind === 'multi') {
        expect(sections.slice(0, 3)).toEqual(['tags', 'folders', 'properties']);
      } else {
        expect(sections).toEqual(['properties']);
      }
      if (entry.unavailableSource) expect(document.querySelector('[data-inspector-anchor="source"]')).toHaveTextContent('—');
      view.unmount();
    }
  });

  it('uses the shared inspector action primitive for Add Tags', () => {
    const view = renderInspector({ target: { kind: 'entity', entityHash: 'entity-1' }, data: entity });

    const addTags = document.querySelector('[data-inspector-action="add-tags"]');
    const autoTag = document.querySelector('[data-inspector-action="auto-tag"]');
    expect(addTags).toHaveTextContent('Add Tags');
    expect(addTags).toHaveAttribute('data-inspector-button-primitive', 'action');
    expect(autoTag).toHaveAttribute('data-inspector-button-primitive', 'action');
    view.unmount();
  });

  it('renders full empty Tags/Folders controls and opens the existing selector portals', () => {
    const view = renderInspector({ target: { kind: 'entity', entityHash: 'entity-1' }, data: entity });
    const store = getDefaultStore();
    const addTags = document.querySelector('[data-inspector-empty-action="add-tags"]');
    const addFolder = document.querySelector('[data-inspector-empty-action="add-folder"]');

    expect(addTags).toHaveTextContent('Add Tags');
    expect(addFolder).toHaveTextContent('Add to Folder');
    expect(addTags).toHaveAttribute('data-inspector-button-variant', 'empty-section');
    expect(addFolder).toHaveAttribute('data-inspector-button-variant', 'empty-section');

    fireEvent.click(addTags!);
    expect(store.get(tagSelectPortalAtom)).toMatchObject({ open: true });
    fireEvent.click(addFolder!);
    expect(store.get(folderPickerPortalAtom)).toMatchObject({ open: true });

    fireEvent.click(screen.getByText('Tags'));
    expect(document.querySelector('[data-inspector-empty-action="add-tags"]')).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Tags'));
    expect(document.querySelector('[data-inspector-empty-action="add-tags"]')).toBeInTheDocument();
    view.unmount();
  });

  it('uses compact icon-only add controls when Tags and Folders are populated', () => {
    const populated = {
      ...entity,
      tags: [{ namespace: 'creator', subtag: 'Example' }],
      folders: [{ folder_id: 1, name: 'References' }],
    };
    const view = renderInspector({ target: { kind: 'entity', entityHash: 'entity-1' }, data: populated });

    const addTags = screen.getByRole('button', { name: 'Add Tags' });
    const addFolder = screen.getByRole('button', { name: 'Add to Folder' });
    expect(addTags).not.toHaveTextContent('Add Tags');
    expect(addFolder).not.toHaveTextContent('Add to Folder');
    expect(addTags.querySelector('svg')).toHaveAttribute('width', '15');
    expect(addFolder.querySelector('svg')).toHaveAttribute('width', '15');
    expect(document.querySelector('[data-inspector-empty-action]')).not.toBeInTheDocument();
    view.unmount();
  });

  it('keeps Auto Tag in inspector scroll flow with its local action variant', () => {
    const entityView = renderInspector({ target: { kind: 'entity', entityHash: 'entity-1' }, data: entity });
    const action = document.querySelector('[data-inspector-action="auto-tag"]');
    expect(action).toHaveAttribute('data-inspector-button-variant', 'flow');
    expect(action?.closest('[data-inspector-scroll-content]')).toBeInTheDocument();
    expect(action?.closest('[data-inspector-section="actions"]')).toBeInTheDocument();
    expect(action?.querySelector('svg')).toHaveAttribute('width', '14');
    entityView.unmount();

    const multiView = renderInspector({ target: { kind: 'multi', count: 2, selectionMode: 'explicit' } });
    expect(document.querySelector('[data-inspector-action="auto-tag"]')).toBeDisabled();
    multiView.unmount();
  });

  it('keeps identity and core anchors for folder, smart-folder, and system scopes', () => {
    for (const kind of ['folder', 'smart_folder', 'system'] as const) {
      const scope = {
        node: { id: `${kind}:1`, kind, name: 'Example', count: 1, meta: {}, icon: null, color: null },
        totalCount: 1, totalSizeBytes: null, searchText: '', previewItems: [], description: null,
        folder: kind === 'folder' ? { folderId: 1, notes: null, autoTags: [], watchEnabled: false } : null,
        smartFolder: kind === 'smart_folder' ? { smartFolderId: 1, parentId: null, notes: null, predicate: null, sortField: null, sortOrder: null } : null,
      };
      const view = renderInspector({ target: { kind: 'scope', nodeId: scope.node.id }, scope });
      expect([...document.querySelectorAll('[data-inspector-anchor]')].map((node) => node.getAttribute('data-inspector-anchor')))
        .toEqual(['name', 'notes']);
      expect([...document.querySelectorAll('[data-inspector-core-property]')].map((node) => node.getAttribute('data-inspector-core-property')))
        .toEqual(['Items']);
      assertNoClassificationSections();
      view.unmount();
    }
  });
});
