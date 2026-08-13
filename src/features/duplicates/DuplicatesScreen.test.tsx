import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { Notifications, notifications } from '@mantine/notifications';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getEntityDetails } from '../../platform/entityApi';
import { getDuplicatePairs, resolveDuplicatePair, scanDuplicates } from '../../platform/duplicateApi';
import type { CanonicalEntityDetails } from '../../shared/types/canonical';
import { DuplicatesScreen } from './DuplicatesScreen';

vi.mock('../../platform/entityApi', () => ({ getEntityDetails: vi.fn() }));
vi.mock('../../platform/duplicateApi', () => ({
  getDuplicatePairs: vi.fn(),
  resolveDuplicatePair: vi.fn(),
  scanDuplicates: vi.fn(),
}));

function details(hash: string, name: string): CanonicalEntityDetails {
  return {
    entity_hash: hash,
    thumbnail_hash: hash,
    entity_kind: 'single',
    name,
    mime_type: 'image/png',
    size_bytes: 1_024,
    pixel_width: 100,
    pixel_height: 100,
    duration_ms: null,
    frame_count: null,
    has_audio: false,
    status: 1,
    rating: null,
    notes: null,
    source_urls: null,
    date_created: '2026-01-01T00:00:00Z',
    date_added: '2026-01-01T00:00:00Z',
    date_modified: '2026-01-01T00:00:00Z',
    dominant_color_hex: null,
    dominant_colors: null,
    perceptual_hash: null,
    tags: [],
    folders: [],
    member_count: null,
    total_size_bytes: null,
  };
}

describe('DuplicatesScreen', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    notifications.clean();
    vi.mocked(getDuplicatePairs).mockResolvedValue({
      items: [{ hash_a: 'left', hash_b: 'right', distance: 1, similarity_pct: 98, status: 'detected' }],
      next_cursor: null,
      has_more: false,
      total: 1,
    });
    vi.mocked(getEntityDetails).mockImplementation(async (hash) => details(hash, hash === 'left' ? 'Left image' : 'Right image'));
    vi.mocked(scanDuplicates).mockResolvedValue({
      candidates_found: 0,
      pairs_inserted: 0,
      reviewable_detected_total: 0,
      reviewable_detected_new: 0,
      total_files: 0,
      files_with_phash: 0,
      files_scanned: 0,
      closest_distance: null,
    });
  });

  it('does not resolve a cross-collection conflict until the user chooses an owner', async () => {
    vi.mocked(getDuplicatePairs)
      .mockResolvedValueOnce({
        items: [{ hash_a: 'left', hash_b: 'right', distance: 1, similarity_pct: 98, status: 'detected' }],
        next_cursor: null,
        has_more: false,
        total: 1,
      })
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false, total: 0 });
    vi.mocked(resolveDuplicatePair)
      .mockResolvedValueOnce({
        status: 'conflict',
        winner_hash: 'left',
        loser_hash: 'right',
        action: 'keep_left',
        affected_folder_ids: [],
        affected_collection_ids: [],
        tags_merged: 0,
        conflict: {
          winner_hash: 'left',
          loser_hash: 'right',
          winner_collection_id: 11,
          loser_collection_id: 22,
        },
        blob_cleanup_pending: false,
        cleanup_error: null,
      })
      .mockResolvedValueOnce({
        status: 'resolved',
        winner_hash: 'left',
        loser_hash: 'right',
        action: 'keep_left',
        affected_folder_ids: [],
        affected_collection_ids: [11, 22],
        tags_merged: 0,
        conflict: null,
        blob_cleanup_pending: false,
        cleanup_error: null,
      });

    const user = userEvent.setup();
    await act(async () => {
      render(<MantineProvider forceColorScheme="dark"><DuplicatesScreen /></MantineProvider>);
    });

    await screen.findByText('Left image');
    expect(screen.queryByText('Duplicate review')).not.toBeInTheDocument();
    expect(screen.queryByText('VS')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Previous pair' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Next pair' })).toBeInTheDocument();
    expect(screen.getByText('98% match')).toBeInTheDocument();
    expect(screen.getByRole('contentinfo')).toHaveTextContent('Not duplicates');
    expect(screen.getByRole('contentinfo')).toHaveTextContent('Smart merge');
    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Keep left' }));
    });

    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    expect(resolveDuplicatePair).toHaveBeenLastCalledWith('keep_left', 'left', 'right', undefined);
    expect(screen.queryByText('Review complete')).not.toBeInTheDocument();

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Collection 11' }));
    });

    await waitFor(() => {
      expect(resolveDuplicatePair).toHaveBeenLastCalledWith('keep_left', 'left', 'right', 11);
      expect(screen.getByText('Review complete')).toBeInTheDocument();
    });
  });

  it('keeps an ambiguous smart merge in review for an explicit choice', async () => {
    vi.mocked(resolveDuplicatePair).mockResolvedValue({
      status: 'quality_ambiguous',
      winner_hash: null,
      loser_hash: null,
      action: 'smart_merge',
      affected_folder_ids: [],
      affected_collection_ids: [],
      tags_merged: 0,
      conflict: null,
      blob_cleanup_pending: false,
      cleanup_error: null,
    });

    const user = userEvent.setup();
    await act(async () => {
      render(
        <MantineProvider forceColorScheme="dark">
          <Notifications />
          <DuplicatesScreen />
        </MantineProvider>,
      );
    });

    await screen.findByText('Left image');
    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Smart merge' }));
    });

    await waitFor(() => expect(resolveDuplicatePair).toHaveBeenCalledTimes(1));
    expect(await screen.findByRole('alert')).toHaveTextContent('No clear quality winner. Choose left or right, or keep both.');
    expect(screen.queryByText('Review complete')).not.toBeInTheDocument();
    expect(getDuplicatePairs).toHaveBeenCalledTimes(1);
  });

  it('warns about pending blob cleanup without blocking resolution', async () => {
    vi.mocked(getDuplicatePairs)
      .mockResolvedValueOnce({
        items: [{ hash_a: 'left', hash_b: 'right', distance: 1, similarity_pct: 98, status: 'detected' }],
        next_cursor: null,
        has_more: false,
        total: 1,
      })
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false, total: 0 });
    vi.mocked(resolveDuplicatePair).mockResolvedValue({
      status: 'resolved',
      winner_hash: 'left',
      loser_hash: 'right',
      action: 'keep_left',
      affected_folder_ids: [],
      affected_collection_ids: [],
      tags_merged: 0,
      conflict: null,
      blob_cleanup_pending: true,
      cleanup_error: 'Cleanup will resume after the blob store is available.',
    });

    const user = userEvent.setup();
    await act(async () => {
      render(
        <MantineProvider forceColorScheme="dark">
          <Notifications />
          <DuplicatesScreen />
        </MantineProvider>,
      );
    });

    await screen.findByText('Left image');
    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Keep left' }));
    });

    expect(await screen.findByRole('alert')).toHaveTextContent('Cleanup will resume after the blob store is available.');
    await waitFor(() => expect(screen.getByText('Review complete')).toBeInTheDocument());
    expect(getDuplicatePairs).toHaveBeenCalledTimes(2);
  });

  it('keeps resolution failures visible while a candidate is displayed', async () => {
    vi.mocked(resolveDuplicatePair).mockRejectedValue(new Error('The pair could not be resolved.'));
    const user = userEvent.setup();

    await act(async () => {
      render(
        <MantineProvider forceColorScheme="dark">
          <Notifications />
          <DuplicatesScreen />
        </MantineProvider>,
      );
    });

    await screen.findByText('Left image');
    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Not duplicates' }));
    });

    expect(await screen.findByRole('alert')).toHaveTextContent('The pair could not be resolved.');
    expect(screen.getByText('Left image')).toBeInTheDocument();
  });

  it('applies one zoom level to both comparison panes', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<MantineProvider forceColorScheme="dark"><DuplicatesScreen /></MantineProvider>);
    });

    await screen.findByText('Left image');
    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Zoom out' }));
    });

    expect(screen.getByText('80%')).toBeInTheDocument();
    expect(screen.getByTestId('left-preview-layers').style.transform).toContain('scale(0.8)');
    expect(screen.getByTestId('right-preview-layers').style.transform).toContain('scale(0.8)');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Fit both images' }));
    });
    expect(screen.getByText('100%')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Fit both images' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('fits different source dimensions independently at the shared 100% zoom', async () => {
    const width = vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(500);
    const height = vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(300);
    vi.mocked(getEntityDetails).mockImplementation(async (hash) => ({
      ...details(hash, hash === 'left' ? 'Left image' : 'Right image'),
      pixel_width: hash === 'left' ? 100 : 200,
      pixel_height: 100,
    }));

    await act(async () => {
      render(<MantineProvider forceColorScheme="dark"><DuplicatesScreen /></MantineProvider>);
    });
    await screen.findByText('Left image');

    expect(screen.getByText('100%')).toBeInTheDocument();
    expect(screen.getByTestId('left-preview-layers').style.transform).toContain('scale(3)');
    expect(screen.getByTestId('right-preview-layers').style.transform).toContain('scale(2.5)');
    width.mockRestore();
    height.mockRestore();
  });

  it('uses a cancelable native wheel listener for linked zoom', async () => {
    await act(async () => {
      render(<MantineProvider forceColorScheme="dark"><DuplicatesScreen /></MantineProvider>);
    });
    await screen.findByText('Left image');
    const event = new WheelEvent('wheel', {
      bubbles: true,
      cancelable: true,
      clientX: 20,
      clientY: 20,
      deltaY: -100,
    });

    act(() => {
      screen.getByTestId('left-preview').dispatchEvent(event);
    });

    expect(event.defaultPrevented).toBe(true);
    expect(screen.queryByText('100%')).not.toBeInTheDocument();
  });

  it('shows an aligned difference composite only while the comparison control is held', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<MantineProvider forceColorScheme="dark"><DuplicatesScreen /></MantineProvider>);
    });

    await screen.findByText('Left image');
    const control = screen.getByRole('button', { name: 'Highlight differences' });
    await act(async () => {
      await user.hover(control);
    });

    expect(screen.getByTestId('left-difference-composite')).toBeInTheDocument();
    expect(screen.getByTestId('right-difference-composite')).toBeInTheDocument();
    expect(control).toHaveAttribute('aria-pressed', 'true');

    await act(async () => {
      await user.unhover(control);
    });
    expect(screen.queryByTestId('left-difference-composite')).not.toBeInTheDocument();
    expect(screen.queryByTestId('right-difference-composite')).not.toBeInTheDocument();
  });

  it('disables decisions while the current pair metadata is loading', async () => {
    vi.mocked(getEntityDetails).mockImplementation(() => new Promise(() => {}));

    await act(async () => {
      render(<MantineProvider forceColorScheme="dark"><DuplicatesScreen /></MantineProvider>);
    });

    expect(await screen.findByRole('button', { name: 'Keep left' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Keep right' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Not duplicates' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Keep both' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Smart merge' })).toBeDisabled();
  });

  it('clears the previous pair metadata while the next pair loads', async () => {
    vi.mocked(getDuplicatePairs).mockResolvedValue({
      items: [
        { hash_a: 'left', hash_b: 'right', distance: 1, similarity_pct: 98, status: 'detected' },
        { hash_a: 'next-left', hash_b: 'next-right', distance: 2, similarity_pct: 97, status: 'detected' },
      ],
      next_cursor: null,
      has_more: false,
      total: 2,
    });
    let resolveNext!: () => void;
    const nextMetadata = new Promise<void>((resolve) => { resolveNext = resolve; });
    vi.mocked(getEntityDetails).mockImplementation(async (hash) => {
      if (hash.startsWith('next-')) {
        await nextMetadata;
      }
      return details(hash, hash === 'left' ? 'Left image' : hash === 'right' ? 'Right image' : `${hash} image`);
    });

    const user = userEvent.setup();
    await act(async () => {
      render(<MantineProvider forceColorScheme="dark"><DuplicatesScreen /></MantineProvider>);
    });
    await screen.findByText('Left image');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Next pair' }));
    });

    expect(screen.queryByText('Left image')).not.toBeInTheDocument();
    expect(screen.getAllByText('Loading metadata...')).toHaveLength(2);

    await act(async () => { resolveNext(); });
    expect(await screen.findByText('next-left image')).toBeInTheDocument();
  });

  it('shows one shared result notification for newly found pairs', async () => {
    vi.mocked(getDuplicatePairs).mockResolvedValue({
      items: [],
      next_cursor: null,
      has_more: false,
      total: 0,
    });
    vi.mocked(scanDuplicates).mockResolvedValue({
      candidates_found: 17,
      pairs_inserted: 17,
      reviewable_detected_total: 17,
      reviewable_detected_new: 17,
      total_files: 100,
      files_with_phash: 100,
      files_scanned: 100,
      closest_distance: 1,
    });
    const user = userEvent.setup();

    await act(async () => {
      render(
        <MantineProvider forceColorScheme="dark">
          <Notifications />
          <DuplicatesScreen />
        </MantineProvider>,
      );
    });
    expect(await screen.findByRole('region', { name: 'No duplicate pairs' })).toBeInTheDocument();
    expect(screen.getByText('Scan the library to find similar images.')).toBeInTheDocument();
    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Scan library' }));
    });

    expect(await screen.findByRole('alert')).toHaveTextContent('Found 17 new review pairs');
  });

  it('keeps the empty state stable and shows progress while the backend scan is running', async () => {
    vi.mocked(getDuplicatePairs).mockResolvedValue({
      items: [],
      next_cursor: null,
      has_more: false,
      total: 0,
    });
    let finishScan!: (summary: Awaited<ReturnType<typeof scanDuplicates>>) => void;
    vi.mocked(scanDuplicates).mockImplementationOnce(() => new Promise((resolve) => {
      finishScan = resolve;
    }));
    const user = userEvent.setup();

    await act(async () => {
      render(
        <MantineProvider forceColorScheme="dark">
          <Notifications />
          <DuplicatesScreen />
        </MantineProvider>,
      );
    });
    await screen.findByRole('region', { name: 'No duplicate pairs' });
    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Scan library' }));
    });

    expect(screen.getByRole('region', { name: 'No duplicate pairs' })).toBeInTheDocument();
    expect(screen.queryByText('Loading duplicate review queue...')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Scan library' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Scan library' })).toBeDisabled();
    expect(screen.queryByRole('button', { name: 'Scanning...' })).not.toBeInTheDocument();
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument();
    expect(await screen.findByRole('progressbar')).toBeInTheDocument();

    await act(async () => {
      finishScan({
        candidates_found: 0,
        pairs_inserted: 0,
        reviewable_detected_total: 0,
        reviewable_detected_new: 0,
        total_files: 0,
        files_with_phash: 0,
        files_scanned: 0,
        closest_distance: null,
      });
    });
    await waitFor(() => expect(screen.queryByRole('progressbar')).not.toBeInTheDocument());
    expect(await screen.findByRole('alert')).toHaveTextContent('Scan complete — no new review pairs');
  });
});
