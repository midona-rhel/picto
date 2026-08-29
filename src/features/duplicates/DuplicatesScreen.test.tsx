import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  getDuplicateItemDetails,
  getDuplicatePairs,
  resolveDuplicatePair,
  scanDuplicates,
  type DuplicatePair,
} from '../../platform/duplicateApi';
import type { CanonicalEntityDetails } from '../../shared/types/canonical';
import { clearNotifications } from '../../shared/lib/notifications';
import { NotificationHost } from '../../shared/ui/NotificationHost/NotificationHost';
import { DuplicatesScreen, DuplicatesToolbar } from './DuplicatesScreen';

const openDefaultAppForHash = vi.hoisted(() => vi.fn());
const openDetailWindow = vi.hoisted(() => vi.fn());
const duplicateInvalidation = vi.hoisted(() => ({
  callback: null as null | (() => void),
}));

vi.mock('../../controllers/filesController', () => ({
  filesController: { openDefaultAppForHash },
}));

vi.mock('../../controllers/windowController', () => ({
  windowController: { openDetailWindow },
}));

vi.mock('../../platform/duplicateApi', () => ({
  getDuplicateItemDetails: vi.fn(),
  getDuplicatePairs: vi.fn(),
  resolveDuplicatePair: vi.fn(),
  scanDuplicates: vi.fn(),
}));

vi.mock('../../runtime/libraryInvalidation', () => ({
  libraryInvalidation: {
    register: vi.fn((_resource: string, callback: () => void) => {
      duplicateInvalidation.callback = callback;
      return () => {
        if (duplicateInvalidation.callback === callback) duplicateInvalidation.callback = null;
      };
    }),
  },
}));

function file(fileId: number, hash: string, width = 100, height = 100) {
  return {
    file_id: fileId,
    file_hash: hash,
    mime_type: 'image/png',
    size_bytes: 1_024,
    pixel_width: width,
    pixel_height: height,
    frame_count: null,
    decoded_information: null,
    has_alpha: null,
  };
}

function pair(decision: DuplicatePair['decision'] = 'NeedsChoice'): DuplicatePair {
  return {
    file_id_a: 1,
    file_id_b: 2,
    distance: 2_000,
    left: {
      file: file(1, 'left'),
      occurrences: [{ media_item_id: 11, root_item_id: 11, collection_id: null }],
    },
    right: {
      file: file(2, 'right'),
      occurrences: [{ media_item_id: 22, root_item_id: 22, collection_id: null }],
    },
    decision,
    status: 'detected',
  };
}

function details(itemId: number, hash: string, name: string): CanonicalEntityDetails {
  return {
    root: {
      root_id: itemId,
      stable_key: `root-${itemId}`,
      kind: 'media',
      name,
      notes: null,
      source_urls: [],
      cover_media_id: itemId,
      imported_at_ms: Date.parse('2026-01-01T00:00:00Z'),
      captured_at_ms: null,
      modified_at_ms: Date.parse('2026-01-01T00:00:00Z'),
      media_count: 1,
      total_size_bytes: 1_024,
    },
    lifecycle: 'active',
    rating: 'unrated',
    folder_ids: [],
    tag_ids: [],
    media: [{
      media_id: itemId,
      media_name: name,
      media_notes: null,
      file_id: itemId,
      file_path: `/media/${hash}`,
      facts: {
        mime: 'image/png',
        size_bytes: 1_024,
        width: 100,
        height: 100,
        duration_ms: null,
        frame_count: null,
        content_hash: hash,
        perceptual_hash: null,
        palette: [],
      },
    }],
    revision: 1,
  };
}

async function renderScreen(withNotifications = false) {
  let result!: ReturnType<typeof render>;
  await act(async () => {
    result = render(
      <MantineProvider forceColorScheme="dark">
        {withNotifications && <NotificationHost />}
        <DuplicatesToolbar />
        <DuplicatesScreen />
      </MantineProvider>,
    );
    await Promise.resolve();
  });
  return result;
}

function setupUser() {
  const user = userEvent.setup();
  return {
    click: (...args: Parameters<typeof user.click>) => act(() => user.click(...args)),
    hover: (...args: Parameters<typeof user.hover>) => act(() => user.hover(...args)),
    unhover: (...args: Parameters<typeof user.unhover>) => act(() => user.unhover(...args)),
  };
}

describe('DuplicatesScreen', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    duplicateInvalidation.callback = null;
    clearNotifications();
    vi.mocked(getDuplicatePairs).mockResolvedValue({
      items: [pair()],
      next_cursor: null,
      has_more: false,
      total: 1,
    });
    vi.mocked(getDuplicateItemDetails).mockImplementation(async (itemId) => (
      itemId === 11 ? details(itemId, 'left', 'Left image') : details(itemId, 'right', 'Right image')
    ));
    vi.mocked(resolveDuplicatePair).mockResolvedValue({
      status: 'resolved',
      choice: 'KeepBoth',
      affected_item_ids: [11, 22],
      freed_file_hash: null,
      receipt: { revision: 2, resources: ['duplicates'], item_ids: [11, 22] },
    });
    vi.mocked(scanDuplicates).mockResolvedValue({
      candidate_count: 0,
      affected_item_ids: [],
      receipt: { revision: 2, resources: ['duplicates'], item_ids: [] },
    });
  });

  it('loads logical details by item ID while rendering physical file hashes', async () => {
    await renderScreen();

    expect(await screen.findByText('Left image')).toBeInTheDocument();
    expect(screen.getByText('Right image')).toBeInTheDocument();
    expect(getDuplicateItemDetails).toHaveBeenCalledWith(11);
    expect(getDuplicateItemDetails).toHaveBeenCalledWith(22);
    expect(screen.getByText('80.0% similar')).toBeInTheDocument();
    expect(screen.getAllByText('Created')).toHaveLength(2);
    expect(screen.getAllByText('Added')).toHaveLength(2);
    expect(screen.getByTestId('left-preview-layers').querySelector('img')).toHaveAttribute(
      'src',
      'media://localhost/thumb/left.jpg',
    );
  });

  it('presents an exact residual match as 100 percent', async () => {
    const equalHashPair = pair();
    equalHashPair.distance = 0;
    vi.mocked(getDuplicatePairs).mockResolvedValueOnce({
      items: [equalHashPair], next_cursor: null, has_more: false, total: 1,
    });

    await renderScreen();

    expect(await screen.findByText('100% similar')).toBeInTheDocument();
  });

  it('preserves a nonzero measured residual instead of rounding it to 100 percent', async () => {
    vi.mocked(getDuplicatePairs).mockResolvedValueOnce({
      items: [{ ...pair(), distance: 1 }],
      next_cursor: null,
      has_more: false,
      total: 1,
    });

    await renderScreen();

    expect(await screen.findByText('99.99% similar')).toBeInTheDocument();
    expect(screen.queryByText('100% similar')).not.toBeInTheDocument();
  });

  it('loads an attached duplicate occurrence through its group root', async () => {
    const groupPair = pair();
    groupPair.left.occurrences = [{
      media_item_id: 11,
      root_item_id: 100,
      collection_id: 100,
    }];
    vi.mocked(getDuplicatePairs).mockResolvedValue({
      items: [groupPair],
      next_cursor: null,
      has_more: false,
      total: 1,
    });
    vi.mocked(getDuplicateItemDetails).mockImplementation(async (itemId) => {
      if (itemId !== 100) return details(itemId, 'right', 'Right image');
      const group = details(11, 'left', 'Member image');
      return {
        ...group,
        root: {
          ...group.root,
          root_id: 100,
          stable_key: 'root-100',
          kind: 'collection',
          name: 'Source post',
        },
      };
    });

    await renderScreen();

    expect(await screen.findByText('Member image')).toBeInTheDocument();
    expect(screen.getByText('Source post')).toBeInTheDocument();
    expect(getDuplicateItemDetails).toHaveBeenCalledWith(100);
    expect(getDuplicateItemDetails).not.toHaveBeenCalledWith(11);
  });

  it('reuses shared entity open actions for duplicate candidates', async () => {
    await renderScreen();
    await screen.findByText('Left image');

    fireEvent.contextMenu(screen.getByText('Left candidate').closest('article')!);
    expect(screen.getByRole('menuitem', { name: /Open with Default App/ })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: /Open in New Window/ })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('menuitem', { name: /Open with Default App/ }));
    expect(openDefaultAppForHash).toHaveBeenCalledWith('left');
  });

  it('resolves a pair through the replacement candidate contract', async () => {
    const user = setupUser();
    await renderScreen();
    await screen.findByText('Left image');

    await user.click(screen.getByRole('button', { name: 'Keep left' }));

    await waitFor(() => expect(resolveDuplicatePair).toHaveBeenLastCalledWith('keep_left', expect.objectContaining({
      file_id_a: 1,
      file_id_b: 2,
    })));
  });

  it('keeps the resolved pair visible until the replacement names are ready', async () => {
    const nextPair = pair('RightBetter');
    nextPair.file_id_a = 3;
    nextPair.file_id_b = 4;
    nextPair.left.file = file(3, 'next-left');
    nextPair.left.occurrences = [{ media_item_id: 33, root_item_id: 33, collection_id: null }];
    nextPair.right.file = file(4, 'next-right');
    nextPair.right.occurrences = [{ media_item_id: 44, root_item_id: 44, collection_id: null }];
    vi.mocked(getDuplicatePairs)
      .mockResolvedValueOnce({ items: [pair()], next_cursor: null, has_more: false, total: 2 })
      .mockResolvedValueOnce({ items: [nextPair], next_cursor: null, has_more: false, total: 1 });

    let releaseNextDetails!: () => void;
    const nextDetailsReady = new Promise<void>((resolve) => { releaseNextDetails = resolve; });
    vi.mocked(getDuplicateItemDetails).mockImplementation(async (itemId) => {
      if (itemId === 33 || itemId === 44) await nextDetailsReady;
      if (itemId === 11) return details(itemId, 'left', 'Left image');
      if (itemId === 22) return details(itemId, 'right', 'Right image');
      if (itemId === 33) return details(itemId, 'next-left', 'Next left image');
      return details(itemId, 'next-right', 'Next right image');
    });
    vi.mocked(resolveDuplicatePair).mockImplementationOnce(async () => {
      duplicateInvalidation.callback?.();
      return {
        status: 'resolved',
        choice: 'KeepBoth',
        affected_item_ids: [11, 22],
        freed_file_hash: null,
        receipt: { revision: 2, resources: ['duplicates'], item_ids: [11, 22] },
      };
    });

    const user = setupUser();
    await renderScreen();
    await screen.findByText('Left image');
    await user.click(screen.getByRole('button', { name: 'Smart merge' }));
    await waitFor(() => expect(getDuplicatePairs).toHaveBeenCalledTimes(2));

    expect(screen.getByText('Left image')).toBeInTheDocument();
    expect(screen.queryByText('next-left')).not.toBeInTheDocument();

    await act(async () => releaseNextDetails());
    expect(await screen.findByText('Next left image')).toBeInTheDocument();
    expect(screen.getByText('Next right image')).toBeInTheDocument();
  });

  it.each([
    ['z', 'keep_left'],
    ['x', 'keep_right'],
    ['b', 'keep_both'],
  ] as const)('resolves with the %s duplicate shortcut', async (key, action) => {
    await renderScreen();
    await screen.findByText('Left image');

    fireEvent.keyDown(window, { key });

    await waitFor(() => expect(resolveDuplicatePair).toHaveBeenLastCalledWith(action, expect.objectContaining({
      file_id_a: 1,
      file_id_b: 2,
    })));
  });

  it('keeps an ambiguous smart merge in review for an explicit choice', async () => {
    const user = setupUser();
    await renderScreen(true);
    await screen.findByText('Left image');

    vi.mocked(resolveDuplicatePair).mockResolvedValueOnce({
      status: 'quality_ambiguous',
      choice: 'KeepBoth',
      affected_item_ids: [],
      freed_file_hash: null,
      receipt: { revision: 2, resources: ['duplicates'], item_ids: [] },
    });
    await user.click(screen.getByRole('button', { name: 'Smart merge' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('No clear quality winner. Choose left or right, or keep both.');
    expect(resolveDuplicatePair).toHaveBeenCalledWith('smart_merge', expect.objectContaining({ decision: 'NeedsChoice' }));
    expect(getDuplicatePairs).toHaveBeenCalledTimes(1);
  });

  it('uses one linked zoom level for both comparison panes', async () => {
    await renderScreen();
    await screen.findByText('Left image');
    const leftLayers = screen.getByTestId('left-preview-layers');
    const rightLayers = screen.getByTestId('right-preview-layers');
    const initialLeftTransform = leftLayers.style.transform;

    fireEvent.wheel(screen.getByTestId('left-preview'), { deltaY: 100, clientX: 0, clientY: 0 });

    expect(leftLayers.style.transform).not.toBe(initialLeftTransform);
    expect(rightLayers.style.transform).toBe(leftLayers.style.transform);
  });

  it('shows the aligned difference composite only while the control is held', async () => {
    const user = setupUser();
    await renderScreen();
    await screen.findByText('Left image');
    const leftTransform = screen.getByTestId('left-preview-layers').style.transform;
    const rightTransform = screen.getByTestId('right-preview-layers').style.transform;
    fireEvent.load(screen.getByTestId('left-preview-layers').querySelector('img[src*="/thumb/"]')!);
    fireEvent.load(screen.getByTestId('right-preview-layers').querySelector('img[src*="/thumb/"]')!);
    const control = screen.getByRole('button', { name: 'Show Difference' });

    await user.hover(control);
    expect(screen.getByTestId('left-difference-composite').style.transform).toBe(leftTransform);
    expect(screen.getByTestId('right-difference-composite').style.transform).toBe(rightTransform);

    await user.unhover(control);
    expect(screen.queryByTestId('left-difference-composite')).not.toBeInTheDocument();
  });

  it('previews the exact Smart Merge survivor while the action is hovered', async () => {
    const automaticPair = pair('LeftBetter');
    vi.mocked(getDuplicatePairs).mockResolvedValue({
      items: [automaticPair],
      next_cursor: null,
      has_more: false,
      total: 1,
    });
    const user = setupUser();
    await renderScreen();
    await screen.findByText('Left image');

    await user.hover(screen.getByRole('button', { name: 'Smart merge' }));

    const leftCard = screen.getByText('Left candidate').closest('article');
    const rightCard = screen.getByText('Right candidate').closest('article');
    expect(leftCard).toHaveAttribute('data-smart-merge-survivor', 'true');
    expect(rightCard).not.toHaveAttribute('data-smart-merge-survivor');

    await user.unhover(screen.getByRole('button', { name: 'Smart merge' }));
    expect(leftCard).not.toHaveAttribute('data-smart-merge-survivor');
    expect(rightCard).not.toHaveAttribute('data-smart-merge-survivor');
  });

  it('places Show Difference immediately after Smart merge in the footer', async () => {
    await renderScreen();
    await screen.findByText('Left image');
    const smartMerge = screen.getByRole('button', { name: 'Smart merge' });
    const showDifference = screen.getByRole('button', { name: 'Show Difference' });

    expect(smartMerge.compareDocumentPosition(showDifference) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('clears the Smart Merge preview when the action is pressed', async () => {
    vi.mocked(getDuplicatePairs).mockResolvedValue({
      items: [pair('RightBetter')],
      next_cursor: null,
      has_more: false,
      total: 1,
    });
    let finishMerge!: (result: Awaited<ReturnType<typeof resolveDuplicatePair>>) => void;
    vi.mocked(resolveDuplicatePair).mockImplementationOnce(() => new Promise((resolve) => { finishMerge = resolve; }));
    const user = setupUser();
    await renderScreen();
    await screen.findByText('Right image');
    const button = screen.getByRole('button', { name: 'Smart merge' });
    const rightCard = screen.getByText('Right candidate').closest('article');

    await user.hover(button);
    expect(rightCard).toHaveAttribute('data-smart-merge-survivor', 'true');
    await user.click(button);
    expect(rightCard).not.toHaveAttribute('data-smart-merge-survivor');

    await act(async () => {
      finishMerge({
        status: 'resolved',
        choice: { KeepFile: { winner_file_id: 2 } },
        affected_item_ids: [11, 22],
        freed_file_hash: 'left',
        receipt: { revision: 3, resources: ['duplicates'], item_ids: [11, 22] },
      });
    });
    expect(rightCard).not.toHaveAttribute('data-smart-merge-survivor');
  });

  it('holds the thumbnail until the full-resolution fade has completed', async () => {
    await renderScreen();
    await screen.findByText('Left image');
    const leftThumbnail = screen.getByTestId('left-preview-layers')
      .querySelector<HTMLImageElement>('img[src="media://localhost/thumb/left.jpg"]')!;
    const rightThumbnail = screen.getByTestId('right-preview-layers')
      .querySelector<HTMLImageElement>('img[src="media://localhost/thumb/right.jpg"]')!;

    expect(document.querySelector('img[data-resolution="full"]')).toBeNull();
    fireEvent.load(leftThumbnail);
    expect(leftThumbnail.className).not.toContain('thumbnailImageReady');
    expect(rightThumbnail.className).not.toContain('thumbnailImageReady');
    expect(document.querySelector('img[data-resolution="full"]')).toBeNull();

    fireEvent.load(rightThumbnail);
    await waitFor(() => {
      expect(leftThumbnail.className).toContain('thumbnailImageReady');
      expect(rightThumbnail.className).toContain('thumbnailImageReady');
    });
    const fullImage = await waitFor(() => {
      const image = document.querySelector<HTMLImageElement>('img[src="media://localhost/file/left.png"]');
      expect(image).not.toBeNull();
      return image!;
    });

    expect(fullImage).toHaveAttribute('loading', 'eager');
    expect(fullImage).toHaveAttribute('decoding', 'async');
    expect(fullImage).toHaveAttribute('data-resolution', 'full');
    fireEvent.load(fullImage);

    await waitFor(() => expect(fullImage.className).toContain('fullImageVisible'));
    expect(screen.getByTestId('left-preview-layers').querySelector('img[src*="/thumb/"]')).not.toBeNull();
    fireEvent.transitionEnd(fullImage, { propertyName: 'opacity' });
    await waitFor(() => {
      expect(screen.getByTestId('left-preview-layers').querySelector('img[src*="/thumb/"]')).toBeNull();
    });
  });

  it('keeps the previous candidate painted until the next thumbnail is decoded', async () => {
    const secondPair = pair();
    secondPair.file_id_a = 3;
    secondPair.file_id_b = 4;
    secondPair.left = {
      file: file(3, 'next-left', 200, 100),
      occurrences: [{ media_item_id: 33, root_item_id: 33, collection_id: null }],
    };
    secondPair.right = {
      file: file(4, 'next-right', 200, 100),
      occurrences: [{ media_item_id: 44, root_item_id: 44, collection_id: null }],
    };
    vi.mocked(getDuplicatePairs).mockResolvedValueOnce({
      items: [pair(), secondPair], next_cursor: null, has_more: false, total: 2,
    });
    vi.mocked(getDuplicateItemDetails).mockImplementation(async (itemId) => {
      if (itemId === 11) return details(itemId, 'left', 'Left image');
      if (itemId === 22) return details(itemId, 'right', 'Right image');
      if (itemId === 33) return details(itemId, 'next-left', 'Next left image');
      return details(itemId, 'next-right', 'Next right image');
    });

    const user = setupUser();
    await renderScreen();
    await screen.findByText('Left image');
    const initialLeftThumbnail = screen.getByTestId('left-preview-layers')
      .querySelector<HTMLImageElement>('img[src="media://localhost/thumb/left.jpg"]')!;
    const initialRightThumbnail = screen.getByTestId('right-preview-layers')
      .querySelector<HTMLImageElement>('img[src="media://localhost/thumb/right.jpg"]')!;
    fireEvent.load(initialLeftThumbnail);
    fireEvent.load(initialRightThumbnail);
    const oldFull = document.querySelector<HTMLImageElement>('img[src="media://localhost/file/left.png"]')!;
    fireEvent.load(oldFull);
    fireEvent.transitionEnd(oldFull, { propertyName: 'opacity' });

    const nextPairButton = screen.getByRole('button', { name: 'Next pair' });
    await user.click(nextPairButton);
    expect(nextPairButton).toHaveFocus();
    expect(nextPairButton).toBeEnabled();
    expect(screen.getByTestId('left-preview-layers').querySelector('img[src="media://localhost/file/left.png"]')).not.toBeNull();
    expect(screen.getByTestId('right-preview-layers').querySelector('img[src="media://localhost/thumb/right.jpg"]')).not.toBeNull();
    const pendingLeft = screen.getByTestId('pending-left-thumbnail');
    const pendingRight = screen.getByTestId('pending-right-thumbnail');
    fireEvent.load(pendingLeft);
    expect(screen.getByTestId('left-preview-layers').querySelector('img[src="media://localhost/file/left.png"]')).not.toBeNull();
    fireEvent.load(pendingRight);

    const leftLayers = screen.getByTestId('left-preview-layers');
    const rightLayers = screen.getByTestId('right-preview-layers');
    await waitFor(() => {
      expect(leftLayers.querySelector('img[src="media://localhost/file/left.png"]')).not.toBeNull();
      expect(leftLayers.querySelector('img[src="media://localhost/thumb/next-left.jpg"]')).not.toBeNull();
      expect(rightLayers.querySelector('img[src="media://localhost/thumb/right.jpg"]')).not.toBeNull();
      expect(rightLayers.querySelector('img[src="media://localhost/thumb/next-right.jpg"]')).not.toBeNull();
    });

    const nextLeftThumbnail = leftLayers.querySelector<HTMLImageElement>('img[src="media://localhost/thumb/next-left.jpg"]')!;
    const nextRightThumbnail = rightLayers.querySelector<HTMLImageElement>('img[src="media://localhost/thumb/next-right.jpg"]')!;
    fireEvent.load(nextLeftThumbnail);
    expect(nextLeftThumbnail.className).not.toContain('thumbnailImageReady');
    expect(leftLayers.querySelector('img[src="media://localhost/file/left.png"]')).not.toBeNull();
    expect(rightLayers.querySelector('img[src="media://localhost/thumb/right.jpg"]')).not.toBeNull();

    fireEvent.load(nextRightThumbnail);
    await waitFor(() => {
      expect(leftLayers.querySelector('img[src="media://localhost/file/left.png"]')).toBeNull();
      expect(rightLayers.querySelector('img[src="media://localhost/thumb/right.jpg"]')).toBeNull();
      expect(nextLeftThumbnail.className).toContain('thumbnailImageReady');
      expect(nextRightThumbnail.className).toContain('thumbnailImageReady');
    });
  });

  it('uses the shared pixel-snapped toolbar glyphs for fit and difference', async () => {
    await renderScreen();
    await screen.findByText('Left image');

    expect(screen.getByRole('button', { name: 'Actual pixels' }).querySelector('[data-toolbar-glyph]')).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Zoom to fit' }).querySelector('[data-toolbar-glyph]')).not.toBeNull();
    const difference = screen.getByRole('button', { name: 'Show Difference' });
    expect(difference).toHaveTextContent('Show Difference');
    expect(difference.closest('footer')).not.toBeNull();
  });

  it('uses the Detail View toolbar order and grouped media navigation', async () => {
    await renderScreen();
    await screen.findByText('Left image');

    const toolbar = screen.getByLabelText('Duplicate review controls');
    const fit = screen.getByRole('button', { name: 'Zoom to fit' });
    const actual = screen.getByRole('button', { name: 'Actual pixels' });
    const previous = screen.getByRole('button', { name: 'Previous pair' });
    const next = screen.getByRole('button', { name: 'Next pair' });

    expect(fit.compareDocumentPosition(actual) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(actual.compareDocumentPosition(previous) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(previous.parentElement).toBe(next.parentElement);
    expect(toolbar.textContent).toContain('1 / 1');
    expect(toolbar.firstElementChild).toHaveTextContent('1 / 1');
    expect(screen.queryByRole('slider', { name: 'Zoom' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Zoom out' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Zoom in' })).not.toBeInTheDocument();
  });

  it('switches the linked pair between actual pixels and zoom to fit', async () => {
    const user = setupUser();
    await renderScreen();
    await screen.findByText('Left image');
    const actual = screen.getByRole('button', { name: 'Actual pixels' });
    const fit = screen.getByRole('button', { name: 'Zoom to fit' });

    expect(fit).toHaveAttribute('aria-pressed', 'true');
    await user.click(actual);
    expect(actual).toHaveAttribute('aria-pressed', 'true');
    expect(fit).toHaveAttribute('aria-pressed', 'false');

    await user.click(fit);
    expect(fit).toHaveAttribute('aria-pressed', 'true');
    expect(actual).toHaveAttribute('aria-pressed', 'false');
  });

  it('keeps decision controls visually stable while logical details are loading', async () => {
    vi.mocked(getDuplicateItemDetails).mockImplementation(() => new Promise(() => {}));
    await renderScreen();

    expect(await screen.findByRole('button', { name: 'Keep left' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Keep right' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Smart merge' })).toBeEnabled();
  });

  it('keeps the empty state stable and reports scan progress', async () => {
    vi.mocked(getDuplicatePairs).mockResolvedValue({ items: [], next_cursor: null, has_more: false, total: 0 });
    let finishScan!: (result: Awaited<ReturnType<typeof scanDuplicates>>) => void;
    vi.mocked(scanDuplicates).mockImplementationOnce(() => new Promise((resolve) => { finishScan = resolve; }));
    const user = setupUser();
    await renderScreen(true);
    await screen.findByRole('region', { name: 'No duplicate pairs' });

    await user.click(screen.getByRole('button', { name: 'Scan library' }));
    expect(screen.getByRole('region', { name: 'No duplicate pairs' })).toBeInTheDocument();
    expect(await screen.findByRole('progressbar')).toBeInTheDocument();

    await act(async () => {
      finishScan({
        candidate_count: 0,
        affected_item_ids: [],
        receipt: { revision: 2, resources: ['duplicates'], item_ids: [] },
      });
    });
    await waitFor(() => expect(screen.queryByRole('progressbar')).not.toBeInTheDocument());
    expect(await screen.findByRole('status')).toHaveTextContent('Scan complete - no new review pairs');
  });
});
