import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { useState } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CollectionSurface } from './CollectionSurface';
import { viewerDisplayControlsAtom, viewerDisplayStateAtom } from '../../state/viewer';

const mocks = vi.hoisted(() => ({
  getItemDetails: vi.fn(),
  detachItems: vi.fn(),
  reorderCollection: vi.fn(),
  setCollectionCover: vi.fn(),
}));

vi.mock('../../controllers/viewerController', () => ({
  viewerController: { getItemDetails: mocks.getItemDetails },
}));
vi.mock('../../platform/entityApi', () => ({
  detachItems: mocks.detachItems,
  reorderCollection: mocks.reorderCollection,
  setCollectionCover: mocks.setCollectionCover,
}));
vi.mock('../../runtime/libraryInvalidation', () => ({
  libraryInvalidation: { register: vi.fn(() => () => {}) },
}));
vi.mock('../viewer/MediaView', () => ({
  MediaView: ({ currentIndex, backLabel }: { currentIndex: number; backLabel?: string }) => (
    <div data-testid="media-view" data-back-label={backLabel}>Viewing {currentIndex}</div>
  ),
}));
vi.mock('../grid/canvas/CanvasGrid', () => ({
  CanvasGrid: (props: {
    items: Array<{ item_id: number; display_file_hash: string }>;
    onReorder?: (ids: number[]) => void;
    onTileClick?: (index: number, item: { item_id: number }, event?: React.MouseEvent) => void;
    onTileContextMenu?: (
      index: number,
      item: { item_id: number },
      position: { x: number; y: number },
    ) => void;
  }) => (
    <div data-testid="canvas-grid">
      {props.items.map((item, index) => (
        <button
          key={item.item_id}
          type="button"
          data-testid={`grid-item-${item.item_id}`}
          onClick={(event) => props.onTileClick?.(index, item, event)}
          onContextMenu={() => props.onTileContextMenu?.(index, item, { x: 10, y: 10 })}
        >
          {item.display_file_hash}
        </button>
      ))}
      <button
        type="button"
        onClick={() => props.onReorder?.([...props.items].reverse().map((item) => item.item_id))}
      >
        Reorder
      </button>
    </div>
  ),
}));
vi.mock('../../shared/ui/ContextMenu', () => ({
  ContextMenu: ({ entries }: { entries: Array<{ label: string; action: () => void }> }) => (
    <div data-testid="context-menu">
      {entries.map((entry) => (
        <button key={entry.label} type="button" onClick={entry.action}>{entry.label}</button>
      ))}
    </div>
  ),
  useContextMenu: () => {
    const [state, setState] = useState<{
      entries: Array<{ label: string; action: () => void }>;
      position: { x: number; y: number };
    } | null>(null);
    return {
      state,
      openAt: (
        position: { x: number; y: number },
        entries: Array<{ label: string; action: () => void }>,
      ) => setState({ position, entries }),
      close: () => setState(null),
    };
  },
}));

function details() {
  return {
    item_id: 7,
    kind: 'collection',
    lifecycle: 'active',
    label: 'Ordered set',
    cover_media_item_id: 1,
    folder_ids: [],
    aggregate_tags: [],
    revision: 1,
    media: [
      media(1, 'one', 'image/png', 0),
      media(2, 'two', 'image/png', 1),
      media(3, 'three', 'video/mp4', 2),
    ],
  };
}

function media(itemId: number, hash: string, mimeType: string, position: number) {
  return {
    media_item_id: itemId,
    file_hash: hash,
    mime_type: mimeType,
    dominant_color_hex: null,
    dominant_colors: [],
    size_bytes: 10,
    pixel_width: 100,
    pixel_height: 200,
    duration_ms: null,
    frame_count: null,
    has_audio: false,
    name: `Item ${itemId}`,
    notes: null,
    rating: null,
    source_urls: [],
    captured_at: null,
    imported_at: '2026-08-23T00:00:00Z',
    position,
    tags: [],
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  getDefaultStore().set(viewerDisplayControlsAtom, null);
  getDefaultStore().set(viewerDisplayStateAtom, null);
  mocks.getItemDetails.mockResolvedValue(details());
  mocks.detachItems.mockResolvedValue({});
  mocks.reorderCollection.mockResolvedValue({});
  mocks.setCollectionCover.mockResolvedValue({});
});

async function enterEditor() {
  await waitFor(() => expect(getDefaultStore().get(viewerDisplayControlsAtom)?.edit).toBeTypeOf('function'));
  act(() => getDefaultStore().get(viewerDisplayControlsAtom)?.edit?.());
}

describe('CollectionSurface', () => {
  it('renders members in persisted order as a document reader', async () => {
    render(<CollectionSurface collectionId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');
    expect([...document.querySelectorAll('[data-collection-member]')].map(
      (element) => element.getAttribute('data-collection-member'),
    )).toEqual(['1', '2', '3']);
  });

  it('opens inline media detail on member double-click', async () => {
    render(<CollectionSurface collectionId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');
    fireEvent.doubleClick(document.querySelector('[data-collection-member="2"]')!);
    const mediaView = await screen.findByTestId('media-view');
    expect(mediaView).toHaveTextContent('Viewing 1');
    expect(mediaView).toHaveAttribute('data-back-label', 'Back to collection');
  });

  it('uses the same inline detail viewer for video members', async () => {
    render(<CollectionSurface collectionId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');
    fireEvent.doubleClick(document.querySelector('[data-collection-member="3"]')!);
    expect(await screen.findByTestId('media-view')).toHaveTextContent('Viewing 2');
  });

  it('sends the complete reordered member list', async () => {
    render(<CollectionSurface collectionId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await enterEditor();
    expect(screen.queryByText(/Drag to reorder/i)).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole('button', { name: 'Reorder' }));
    await waitFor(() => expect(mocks.reorderCollection).toHaveBeenCalledWith({
      collection_id: 7,
      media_item_ids: [3, 2, 1],
    }));
  });

  it('removes multiple selected members from the collection', async () => {
    render(<CollectionSurface collectionId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await enterEditor();
    fireEvent.click(await screen.findByTestId('grid-item-1'));
    fireEvent.click(await screen.findByTestId('grid-item-2'), { metaKey: true });
    fireEvent.contextMenu(screen.getByTestId('grid-item-2'));
    fireEvent.click(await screen.findByRole('button', { name: 'Remove 2 from Collection' }));
    await waitFor(() => expect(mocks.detachItems).toHaveBeenCalledWith({
      collection_id: 7,
      media_item_ids: [1, 2],
    }));
  });

  it('sets a single selected member as the cover', async () => {
    render(<CollectionSurface collectionId={7} initialMode="editor" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    fireEvent.contextMenu(await screen.findByTestId('grid-item-2'));
    fireEvent.click(await screen.findByRole('button', { name: 'Set as Cover' }));
    await waitFor(() => expect(mocks.setCollectionCover).toHaveBeenCalledWith({
      collection_id: 7,
      media_item_id: 2,
    }));
  });

  it('uses arrow keys to navigate the root detail sequence', async () => {
    const navigate = vi.fn();
    render(<CollectionSurface collectionId={7} rootCurrentIndex={1} rootTotal={3} onNavigateRoot={navigate} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');
    fireEvent.keyDown(window, { key: 'ArrowLeft' });
    fireEvent.keyDown(window, { key: 'ArrowRight' });
    expect(navigate.mock.calls).toEqual([[-1], [1]]);
  });
});
