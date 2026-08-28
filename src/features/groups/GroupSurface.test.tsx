import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { useState } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { GroupSurface, retainWarmGroupMedia } from './GroupSurface';
import { viewerDisplayControlsAtom, viewerDisplayStateAtom } from '../../state/viewer';
import type { CanonicalEntityDetails } from '../../shared/types/canonical';

const mocks = vi.hoisted(() => ({
  getItemDetails: vi.fn(),
  detachItems: vi.fn(),
  reorderGroup: vi.fn(),
  ungroup: vi.fn(),
  openDefaultAppForHash: vi.fn(),
  revealHashInFolder: vi.fn(),
  openDetailWindow: vi.fn(),
  copyFileForHash: vi.fn(),
  copyFilePath: vi.fn(),
  copyText: vi.fn(),
  regenerateThumbnailsBatch: vi.fn(),
  getTagsById: vi.fn(),
  openLibraryCoverPicker: vi.fn(() => Promise.resolve()),
  detailMediaRenderer: vi.fn(),
}));

vi.mock('../../controllers/viewerController', () => ({
  viewerController: { getItemDetails: mocks.getItemDetails },
}));
vi.mock('../../platform/entityApi', () => ({
  detachItems: mocks.detachItems,
  reorderGroup: mocks.reorderGroup,
  ungroup: mocks.ungroup,
}));
vi.mock('../../runtime/libraryInvalidation', () => ({
  libraryInvalidation: { register: vi.fn(() => () => {}) },
}));
vi.mock('../../controllers/filesController', () => ({
  filesController: {
    openDefaultAppForHash: mocks.openDefaultAppForHash,
    revealHashInFolder: mocks.revealHashInFolder,
    copyFileForHash: mocks.copyFileForHash,
    copyFilePath: mocks.copyFilePath,
    copyText: mocks.copyText,
    regenerateThumbnailsBatch: mocks.regenerateThumbnailsBatch,
  },
}));
vi.mock('../../controllers/tagsController', () => ({
  tagsController: { getById: mocks.getTagsById },
}));
vi.mock('../../controllers/windowController', () => ({
  windowController: { openDetailWindow: mocks.openDetailWindow },
}));
vi.mock('../library/libraryAppearance', () => ({
  openCurrentLibraryCoverPicker: mocks.openLibraryCoverPicker,
}));
vi.mock('../viewer/MediaView', () => ({
  MediaView: ({ currentIndex, backLabel, onClose }: { currentIndex: number; backLabel?: string; onClose: () => void }) => (
    <div data-testid="media-view" data-back-label={backLabel}>
      Viewing {currentIndex}
      <button type="button" onClick={onClose}>Back from member</button>
    </div>
  ),
}));
vi.mock('../viewer/QuickLook', () => ({
  QuickLook: ({ currentIndex, onClose }: { currentIndex: number; onClose: () => void }) => (
    <div data-testid="quick-look">
      Quick looking {currentIndex}
      <button type="button" onClick={onClose}>Close quick look</button>
    </div>
  ),
}));
vi.mock('../viewer/document/DetailMediaRenderer', () => ({
  DetailMediaRenderer: (props: { hash: string; mimeType: string; mediaKeyboardShortcutsEnabled?: boolean }) => {
    mocks.detailMediaRenderer(props);
    return <div data-testid={`detail-renderer-${props.hash}`} />;
  },
}));
vi.mock('../grid/canvas/CanvasGrid', () => ({
  CanvasGrid: (props: {
    items: Array<{ root_id: number; content_hash: string }>;
    onReorder?: (ids: number[]) => void;
    onTileClick?: (index: number, item: { root_id: number }, event?: React.MouseEvent) => void;
    onTileContextMenu?: (
      index: number,
      item: { root_id: number },
      position: { x: number; y: number },
    ) => void;
    onMarqueeSelectionChange?: (selection: { itemIds: Set<number>; folderNodeIds: Set<string> }) => void;
  }) => (
    <div data-testid="canvas-grid">
      {props.items.map((item, index) => (
        <button
          key={item.root_id}
          type="button"
          data-testid={`grid-item-${item.root_id}`}
          onClick={(event) => props.onTileClick?.(index, item, event)}
          onContextMenu={() => props.onTileContextMenu?.(index, item, { x: 10, y: 10 })}
        >
          {item.content_hash}
        </button>
      ))}
      <button
        type="button"
        onClick={() => props.onReorder?.([...props.items].reverse().map((item) => item.root_id))}
      >
        Reorder
      </button>
      <button type="button" onClick={() => props.onMarqueeSelectionChange?.({ itemIds: new Set([1, 2]), folderNodeIds: new Set() })}>
        Marquee first two
      </button>
    </div>
  ),
}));
vi.mock('../../shared/ui/ContextMenu', () => {
  type TestMenuEntry = {
    label?: string;
    action?: () => void;
    separator?: true;
    submenu?: true;
    children?: TestMenuEntry[];
  };
  const TestContextMenu = ({ entries }: { entries: TestMenuEntry[] }) => {
    const [openSubmenu, setOpenSubmenu] = useState<string | null>(null);
    const children = entries.find((entry) => entry.label === openSubmenu)?.children ?? [];
    const renderEntry = (entry: TestMenuEntry, index: number, prefix: string) => entry.separator
      ? <hr key={`${prefix}-separator-${index}`} />
      : (
        <button
          key={`${prefix}-${entry.label}-${index}`}
          type="button"
          onClick={entry.submenu ? () => setOpenSubmenu(entry.label ?? null) : entry.action}
        >
          {entry.label}
        </button>
      );
    return (
      <div data-testid="context-menu">
        {entries.map((entry, index) => renderEntry(entry, index, 'root'))}
        {children.map((entry, index) => renderEntry(entry, index, 'submenu'))}
      </div>
    );
  };

  return {
  ContextMenu: TestContextMenu,
  useContextMenu: () => {
    const [state, setState] = useState<{
      entries: TestMenuEntry[];
      position: { x: number; y: number };
    } | null>(null);
    return {
      state,
      openAt: (
        position: { x: number; y: number },
        entries: TestMenuEntry[],
      ) => setState({ position, entries }),
      close: () => setState(null),
    };
  },
  };
});

function details(): CanonicalEntityDetails {
  return {
    root: {
      root_id: 7,
      stable_key: 'root-7',
      kind: 'collection',
      name: 'Ordered set',
      notes: null,
      source_urls: [],
      cover_media_id: 1,
      imported_at_ms: 1,
      captured_at_ms: null,
      modified_at_ms: 1,
      media_count: 3,
      total_size_bytes: 30,
    },
    lifecycle: 'active',
    rating: 'unrated',
    folder_ids: [],
    tag_ids: [],
    revision: 1,
    media: [
      media(1, 'one', 'image/png', 0),
      media(2, 'two', 'image/png', 1),
      media(3, 'three', 'video/mp4', 2),
    ],
  };
}

function media(itemId: number, hash: string, mimeType: string, _position: number) {
  return {
    media_id: itemId,
    media_name: `Item ${itemId}`,
    file_id: itemId,
    file_path: `/media/${hash}`,
    facts: {
      mime: mimeType,
      size_bytes: 10,
      width: 100,
      height: 200,
      duration_ms: null,
      frame_count: null,
      content_hash: hash,
      perceptual_hash: null,
      palette: [],
    },
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  getDefaultStore().set(viewerDisplayControlsAtom, null);
  getDefaultStore().set(viewerDisplayStateAtom, null);
  mocks.getItemDetails.mockResolvedValue(details());
  mocks.detachItems.mockResolvedValue({});
  mocks.reorderGroup.mockResolvedValue({});
  mocks.ungroup.mockResolvedValue({});
  mocks.getTagsById.mockImplementation((tagIds: number[]) => Promise.resolve(tagIds.map((tagId) => ({
    tag_id: tagId,
    namespace_id: tagId === 1 ? 1 : 0,
    namespace: tagId === 1 ? 'creator' : '',
    subname: tagId === 1 ? 'alice' : 'favorite',
    active_count: 1,
    assignment_count: 1,
  }))));
});

async function enterEditor() {
  await waitFor(() => expect(getDefaultStore().get(viewerDisplayControlsAtom)?.edit).toBeTypeOf('function'));
  act(() => getDefaultStore().get(viewerDisplayControlsAtom)?.edit?.());
}

describe('GroupSurface', () => {
  it('does not expose a loading screen while group details are fetched', () => {
    mocks.getItemDetails.mockReturnValue(new Promise(() => {}));
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);

    expect(screen.queryByText(/loading group/i)).not.toBeInTheDocument();
  });

  it('keeps at most 100 full-media members using least-recently-viewed eviction', () => {
    let order = Array.from({ length: 100 }, (_, index) => index + 1);
    order = retainWarmGroupMedia(order, 1);
    order = retainWarmGroupMedia(order, 101);

    expect(order).toHaveLength(100);
    expect(order).not.toContain(2);
    expect(order).toContain(1);
    expect(order[order.length - 1]).toBe(101);
  });

  it('renders members in persisted order as a document reader', async () => {
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');
    expect([...document.querySelectorAll('[data-group-member]')].map(
      (element) => element.getAttribute('data-group-member'),
    )).toEqual(['1', '2', '3']);
  });

  it('does not replace the application toolbar while presented inside Quick Look', async () => {
    render(<GroupSurface groupId={7} presentation="quicklook" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');
    expect(getDefaultStore().get(viewerDisplayControlsAtom)).toBeNull();
    expect(getDefaultStore().get(viewerDisplayStateAtom)).toBeNull();
  });

  it('opens inline media detail on member double-click', async () => {
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');
    fireEvent.doubleClick(document.querySelector('[data-group-member="2"]')!);
    const mediaView = await screen.findByTestId('media-view');
    expect(mediaView).toHaveTextContent('Viewing 1');
    expect(mediaView).toHaveAttribute('data-back-label', 'Back to group');
  });

  it('uses the same inline detail viewer for video members', async () => {
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');
    fireEvent.doubleClick(document.querySelector('[data-group-member="3"]')!);
    expect(await screen.findByTestId('media-view')).toHaveTextContent('Viewing 2');
  });

  it('uses the canonical media renderer for video members in the group reader', async () => {
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    expect(await screen.findByTestId('detail-renderer-three')).toBeInTheDocument();
    expect(mocks.detailMediaRenderer).toHaveBeenCalledWith(expect.objectContaining({
      hash: 'three',
      mimeType: 'video/mp4',
      mediaKeyboardShortcutsEnabled: false,
      mediaAutoPlay: false,
      mediaLoop: false,
      mediaMuted: false,
    }));
    expect(document.querySelector('[data-group-member="3"] > video')).toBeNull();
  });

  it('uses the shared media actions for a member without enabling reader selection', async () => {
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');

    fireEvent.contextMenu(document.querySelector('[data-group-member="1"]')!);

    fireEvent.click(await screen.findByRole('button', { name: 'More' }));
    expect(await screen.findByRole('button', { name: 'Set as Library Cover' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add Tags' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Select All' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Set as Library Cover' }));
    expect(mocks.openLibraryCoverPicker).toHaveBeenCalledWith(expect.objectContaining({
      media_item_id: 1,
      file_hash: 'one',
    }));
  });

  it('shows member thumbnails before revealing decoded full images', async () => {
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');

    const memberImages = [...document.querySelectorAll('[data-group-member] img')];
    const thumbnails = memberImages.filter((image) => image.getAttribute('src')?.includes('/thumb/'));
    expect(thumbnails).toHaveLength(3);
    expect(memberImages.filter((image) => image.getAttribute('src')?.includes('/file/'))).toHaveLength(0);

    await waitFor(() => expect(document.querySelectorAll('[data-group-member] img[src*="/file/"]')).toHaveLength(2));
    const fullImages = [...document.querySelectorAll<HTMLImageElement>('[data-group-member] img[src*="/file/"]')];
    expect(fullImages).toHaveLength(2);
    expect(fullImages[0].className).not.toContain('fullImageVisible');
    fireEvent.load(fullImages[0]);
    expect(fullImages[0].className).toContain('fullImageVisible');
    expect(getComputedStyle(fullImages[0]).position).toBe('absolute');
    expect(getComputedStyle(thumbnails[0]).position).toBe('absolute');
    expect(getComputedStyle(fullImages[0]).objectFit).toBe('fill');
    expect(getComputedStyle(thumbnails[0]).objectFit).toBe('fill');
    expect(fullImages[0].parentElement).toBe(thumbnails[0].parentElement);
  });

  it('requests full group media only after 200ms of continuous visibility', async () => {
    const observed = new Map<Element, IntersectionObserverCallback>();
    class TestIntersectionObserver {
      readonly root = null;
      readonly rootMargin = '';
      readonly thresholds = [0.01];
      constructor(private readonly callback: IntersectionObserverCallback) {}
      observe = (target: Element) => { observed.set(target, this.callback); };
      unobserve = (target: Element) => { observed.delete(target); };
      disconnect = () => {};
      takeRecords = () => [];
    }
    vi.stubGlobal('IntersectionObserver', TestIntersectionObserver);

    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');
    const frame = document.querySelector('[data-group-member="1"] > div')!;
    const callback = observed.get(frame)!;
    const entry = (isIntersecting: boolean) => ({ isIntersecting, target: frame } as IntersectionObserverEntry);

    act(() => callback([entry(true)], {} as IntersectionObserver));
    await new Promise((resolve) => setTimeout(resolve, 100));
    act(() => callback([entry(false)], {} as IntersectionObserver));
    await new Promise((resolve) => setTimeout(resolve, 130));
    expect(frame.querySelector('img[src*="/file/"]')).toBeNull();

    act(() => callback([entry(true)], {} as IntersectionObserver));
    await waitFor(() => expect(frame.querySelector('img[src*="/file/"]')).not.toBeNull());
    vi.unstubAllGlobals();
  });

  it.each([' ', 'Enter'])('closes the group reader with the shared viewer toggle %p', async (key) => {
    const onClose = vi.fn();
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={onClose} />);
    await screen.findByLabelText('Ordered set');
    fireEvent.keyDown(window, { key, code: key === ' ' ? 'Space' : 'Enter' });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('opens Quick Look for the selected group member from the editor', async () => {
    render(<GroupSurface groupId={7} initialMode="editor" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(await screen.findByTestId('grid-item-2'));
    fireEvent.keyDown(window, { key: ' ', code: 'Space' });
    expect(await screen.findByTestId('quick-look')).toHaveTextContent('Quick looking 1');
  });

  it('sends the complete reordered member list', async () => {
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await enterEditor();
    expect(screen.queryByText(/Drag to reorder/i)).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole('button', { name: 'Reorder' }));
    await waitFor(() => expect(mocks.reorderGroup).toHaveBeenCalledWith({
      collection_id: 7,
      media_ids: [3, 2, 1],
    }));
  });

  it('removes multiple selected members from the group', async () => {
    const onClose = vi.fn();
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={onClose} />);
    await enterEditor();
    fireEvent.click(await screen.findByTestId('grid-item-1'));
    fireEvent.click(await screen.findByTestId('grid-item-2'), { metaKey: true });
    fireEvent.contextMenu(screen.getByTestId('grid-item-2'));
    fireEvent.click(await screen.findByRole('button', { name: 'Remove 2 from Group' }));
    await waitFor(() => expect(mocks.detachItems).toHaveBeenCalledWith({
      collection_id: 7,
      media_ids: [1, 2],
      target_lifecycle: null,
    }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('moves selected members directly to trash from the group menu', async () => {
    render(<GroupSurface groupId={7} initialMode="editor" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    const member = await screen.findByTestId('grid-item-2');
    fireEvent.contextMenu(member);

    const remove = await screen.findByRole('button', { name: 'Remove from Group' });
    const trash = screen.getByRole('button', { name: 'Move to Trash' });
    expect(remove.compareDocumentPosition(trash) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    fireEvent.click(trash);
    await waitFor(() => expect(mocks.detachItems).toHaveBeenCalledWith({
      collection_id: 7,
      media_ids: [2],
      target_lifecycle: 'trash',
    }));
  });

  it('marquee-selects members for group-native bulk actions', async () => {
    render(<GroupSurface groupId={7} initialMode="editor" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByTestId('grid-item-1');

    fireEvent.click(screen.getByRole('button', { name: 'Marquee first two' }));
    fireEvent.contextMenu(screen.getByTestId('grid-item-2'));
    fireEvent.click(await screen.findByRole('button', { name: 'More' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Regenerate 2 Thumbnails' }));
    expect(mocks.regenerateThumbnailsBatch).toHaveBeenCalledWith(['one', 'two']);
  });

  it('removes the selected editor members with Delete', async () => {
    const onClose = vi.fn();
    render(<GroupSurface groupId={7} initialMode="editor" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={onClose} />);
    await screen.findByTestId('grid-item-1');
    fireEvent.click(screen.getByRole('button', { name: 'Marquee first two' }));
    fireEvent.keyDown(window, { key: 'Delete' });
    await waitFor(() => expect(mocks.detachItems).toHaveBeenCalledWith({
      collection_id: 7,
      media_ids: [1, 2],
      target_lifecycle: null,
    }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('does not expose a separate cover choice', async () => {
    render(<GroupSurface groupId={7} initialMode="editor" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    fireEvent.contextMenu(await screen.findByTestId('grid-item-2'));
    expect(screen.queryByRole('button', { name: 'Set as Cover' })).not.toBeInTheDocument();
  });

  it('reuses shared open actions for a single group member', async () => {
    render(<GroupSurface groupId={7} initialMode="editor" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    fireEvent.contextMenu(await screen.findByTestId('grid-item-2'));
    fireEvent.click(await screen.findByRole('button', { name: 'Open with Default App' }));
    expect(mocks.openDefaultAppForHash).toHaveBeenCalledWith('two');
  });

  it('does not select in the reader and applies its menu to one member', async () => {
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');
    const first = document.querySelector('[data-group-member="1"]')!;
    const second = document.querySelector('[data-group-member="2"]')!;

    fireEvent.click(first);
    fireEvent.click(second, { metaKey: true });
    expect(document.querySelectorAll('[data-selected="true"]')).toHaveLength(0);

    fireEvent.contextMenu(second);
    expect(screen.queryByRole('button', { name: 'Select All' })).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole('button', { name: 'Remove from Group' }));
    await waitFor(() => expect(mocks.detachItems).toHaveBeenCalledWith({
      collection_id: 7,
      media_ids: [2],
      target_lifecycle: null,
    }));
  });

  it('keeps contiguous shift selection inside the editor', async () => {
    const onClose = vi.fn();
    render(<GroupSurface groupId={7} initialMode="editor" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={onClose} />);
    const first = await screen.findByTestId('grid-item-1');
    const third = screen.getByTestId('grid-item-3');

    fireEvent.click(first);
    fireEvent.click(third, { shiftKey: true });
    fireEvent.contextMenu(third);
    expect(await screen.findByRole('button', { name: 'Remove 3 from Group' })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('returns directly to the grid when editing was opened from the grid', async () => {
    const onClose = vi.fn();
    render(<GroupSurface groupId={7} initialMode="editor" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={onClose} />);
    await screen.findByTestId('grid-item-1');
    const controls = getDefaultStore().get(viewerDisplayControlsAtom);
    expect(controls?.backLabel).toBe('Back to grid');
    act(() => controls?.close());
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.getByTestId('canvas-grid')).toBeInTheDocument();
  });

  it('returns to the grid when editing was entered from the group reader', async () => {
    const onClose = vi.fn();
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={onClose} />);
    await enterEditor();
    const controls = getDefaultStore().get(viewerDisplayControlsAtom);
    expect(controls?.backLabel).toBe('Back to grid');
    act(() => controls?.close());
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('returns from an opened member to the group reader, not the grid', async () => {
    const onClose = vi.fn();
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={onClose} />);
    await screen.findByLabelText('Ordered set');
    fireEvent.doubleClick(document.querySelector('[data-group-member="2"]')!);
    await screen.findByTestId('media-view');
    fireEvent.click(screen.getByRole('button', { name: 'Back from member' }));
    expect(onClose).not.toHaveBeenCalled();
    expect(await screen.findByRole('main')).toBeInTheDocument();
  });

  it('adds truthful file and thumbnail actions to the member menu', async () => {
    render(<GroupSurface groupId={7} initialMode="editor" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    fireEvent.contextMenu(await screen.findByTestId('grid-item-2'));

    fireEvent.click(await screen.findByRole('button', { name: 'Copy File Path' }));
    expect(mocks.copyFilePath).toHaveBeenCalledWith('two');

    fireEvent.contextMenu(await screen.findByTestId('grid-item-2'));
    fireEvent.click(await screen.findByRole('button', { name: 'More' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Regenerate Thumbnail' }));
    expect(mocks.regenerateThumbnailsBatch).toHaveBeenCalledWith(['two']);
  });

  it('copies tags owned by the collection root', async () => {
    const next = details();
    next.tag_ids = [1, 2];
    mocks.getItemDetails.mockResolvedValue(next);
    render(<GroupSurface groupId={7} initialMode="editor" rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={vi.fn()} />);
    fireEvent.contextMenu(await screen.findByTestId('grid-item-2'));
    fireEvent.click(await screen.findByRole('button', { name: 'Copy Tags' }));
    await waitFor(() => expect(mocks.copyText).toHaveBeenCalledWith('["creator:alice","favorite"]'));
  });

  it('confirms before dissolving the whole group', async () => {
    const onClose = vi.fn();
    render(<GroupSurface groupId={7} rootCurrentIndex={0} rootTotal={1} onNavigateRoot={vi.fn()} onClose={onClose} />);
    await screen.findByLabelText('Ordered set');
    fireEvent.contextMenu(screen.getByRole('main'));
    fireEvent.click(await screen.findByRole('button', { name: 'Ungroup...' }));

    const confirm = getDefaultStore().get((await import('../../state/modals')).confirmModalAtom);
    expect(confirm).toMatchObject({ open: true, confirmLabel: 'Ungroup' });
    await act(async () => { confirm.onConfirm(); });
    await waitFor(() => expect(mocks.ungroup).toHaveBeenCalledWith(7));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('uses arrow keys to navigate the root detail sequence', async () => {
    const navigate = vi.fn();
    render(<GroupSurface groupId={7} rootCurrentIndex={1} rootTotal={3} onNavigateRoot={navigate} onClose={vi.fn()} />);
    await screen.findByLabelText('Ordered set');
    fireEvent.keyDown(window, { key: 'ArrowLeft' });
    fireEvent.keyDown(window, { key: 'ArrowRight' });
    expect(navigate.mock.calls).toEqual([[-1], [1]]);
  });
});
