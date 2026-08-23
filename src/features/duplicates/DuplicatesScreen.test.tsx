import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { Notifications, notifications } from '@mantine/notifications';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getEntityDetails } from '../../platform/entityApi';
import { getDuplicatePairs, resolveDuplicatePair, scanDuplicates } from '../../platform/duplicateApi';
import type { CanonicalEntityDetails } from '../../shared/types/canonical';
import { DuplicatesScreen, DuplicatesToolbar } from './DuplicatesScreen';

vi.mock('../../platform/entityApi', () => ({ getEntityDetails: vi.fn() }));
vi.mock('../../platform/duplicateApi', () => ({
  getDuplicatePairs: vi.fn(),
  resolveDuplicatePair: vi.fn(),
  scanDuplicates: vi.fn(),
}));

function details(hash: string, name: string): CanonicalEntityDetails {
  return {
    entity_hash: hash,
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
  };
}

function DuplicateTestSurface() {
  return (
    <>
      <DuplicatesToolbar />
      <DuplicatesScreen />
    </>
  );
}

describe('DuplicatesScreen', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    notifications.clean();
    vi.mocked(getDuplicatePairs).mockResolvedValue({
      items: [{ hash_a: 'left', hash_b: 'right', smart_winner_hash: 'left', distance: 1, similarity_pct: 98, status: 'detected' }],
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

  it('resolves a duplicate using direct media ownership', async () => {
    vi.mocked(getDuplicatePairs)
      .mockResolvedValueOnce({
        items: [{ hash_a: 'left', hash_b: 'right', smart_winner_hash: 'left', distance: 1, similarity_pct: 98, status: 'detected' }],
        next_cursor: null,
        has_more: false,
        total: 1,
      })
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false, total: 0 });
    vi.mocked(resolveDuplicatePair).mockResolvedValueOnce({
        status: 'resolved',
        winner_hash: 'left',
        loser_hash: 'right',
        action: 'keep_left',
        affected_folder_ids: [],
        tags_merged: 0,
        blob_cleanup_pending: false,
        cleanup_error: null,
      });

    const user = userEvent.setup();
    await act(async () => {
      render(<MantineProvider forceColorScheme="dark"><DuplicateTestSurface /></MantineProvider>);
    });

    await screen.findByText('Left image');
    expect(screen.queryByText('Duplicate review')).not.toBeInTheDocument();
    expect(screen.queryByText('VS')).not.toBeInTheDocument();
    const toolbar = screen.getByLabelText('Duplicate review controls');
    expect(toolbar).toContainElement(screen.getByRole('button', { name: 'Previous pair' }));
    expect(screen.getByRole('button', { name: 'Next pair' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Re-scan library' })).not.toBeInTheDocument();
    expect(screen.getByText('98% match')).toBeInTheDocument();
    expect(screen.getByRole('contentinfo')).toHaveTextContent('Not duplicates');
    expect(screen.getByRole('contentinfo')).toHaveTextContent('Smart merge');
    await act(async () => {
      await user.hover(screen.getByRole('button', { name: 'Smart merge' }));
    });
    expect(screen.getByText('Keep file · combine metadata')).toBeInTheDocument();
    expect(screen.getByText('Transfer metadata · remove file')).toBeInTheDocument();
    await act(async () => {
      await user.unhover(screen.getByRole('button', { name: 'Smart merge' }));
    });
    expect(screen.queryByText('Keep file · combine metadata')).not.toBeInTheDocument();
    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Keep left' }));
    });

    await waitFor(() => {
      expect(resolveDuplicatePair).toHaveBeenLastCalledWith('keep_left', 'left', 'right');
      expect(screen.getByText('Review complete')).toBeInTheDocument();
    });
  });

  it('previews the earlier candidate when an older backend omits the smart winner', async () => {
    vi.mocked(getDuplicatePairs).mockResolvedValue({
      items: [{ hash_a: 'left', hash_b: 'right', smart_winner_hash: null, distance: 1, similarity_pct: 98, status: 'detected' }],
      next_cursor: null,
      has_more: false,
      total: 1,
    });

    const user = userEvent.setup();
    await act(async () => {
      render(
        <MantineProvider forceColorScheme="dark">
          <Notifications />
          <DuplicateTestSurface />
        </MantineProvider>,
      );
    });

    await screen.findByText('Left image');
    await act(async () => {
      await user.hover(screen.getByRole('button', { name: 'Smart merge' }));
    });
    expect(screen.getByText('Left image').closest('article')).toHaveAttribute('data-merge-preview-role', 'winner');
    expect(screen.getByText('Right image').closest('article')).toHaveAttribute('data-merge-preview-role', 'loser');
  });

  it('warns about pending blob cleanup without blocking resolution', async () => {
    vi.mocked(getDuplicatePairs)
      .mockResolvedValueOnce({
        items: [{ hash_a: 'left', hash_b: 'right', smart_winner_hash: 'left', distance: 1, similarity_pct: 98, status: 'detected' }],
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
      tags_merged: 0,
      blob_cleanup_pending: true,
      cleanup_error: 'Cleanup will resume after the blob store is available.',
    });

    const user = userEvent.setup();
    await act(async () => {
      render(
        <MantineProvider forceColorScheme="dark">
          <Notifications />
          <DuplicateTestSurface />
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
          <DuplicateTestSurface />
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
      render(<MantineProvider forceColorScheme="dark"><DuplicateTestSurface /></MantineProvider>);
    });

    await screen.findByText('Left image');
    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Zoom out' }));
    });

    expect(screen.getByRole('slider', { name: 'Zoom' })).toHaveValue('80');
    expect(screen.getByTestId('left-preview-layers').style.transform).toContain('scale(0.8)');
    expect(screen.getByTestId('right-preview-layers').style.transform).toContain('scale(0.8)');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Fit both images' }));
    });
    expect(screen.getByRole('slider', { name: 'Zoom' })).toHaveValue('100');
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
      render(<MantineProvider forceColorScheme="dark"><DuplicateTestSurface /></MantineProvider>);
    });
    await screen.findByText('Left image');

    expect(screen.getByRole('slider', { name: 'Zoom' })).toHaveValue('100');
    expect(screen.getByTestId('left-preview-layers').style.transform).toContain('scale(3)');
    expect(screen.getByTestId('right-preview-layers').style.transform).toContain('scale(2.5)');
    width.mockRestore();
    height.mockRestore();
  });

  it('uses a cancelable native wheel listener for linked zoom', async () => {
    await act(async () => {
      render(<MantineProvider forceColorScheme="dark"><DuplicateTestSurface /></MantineProvider>);
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
      render(<MantineProvider forceColorScheme="dark"><DuplicateTestSurface /></MantineProvider>);
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
      render(<MantineProvider forceColorScheme="dark"><DuplicateTestSurface /></MantineProvider>);
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
        { hash_a: 'left', hash_b: 'right', smart_winner_hash: 'left', distance: 1, similarity_pct: 98, status: 'detected' },
        { hash_a: 'next-left', hash_b: 'next-right', smart_winner_hash: 'next-right', distance: 2, similarity_pct: 97, status: 'detected' },
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
      render(<MantineProvider forceColorScheme="dark"><DuplicateTestSurface /></MantineProvider>);
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
          <DuplicateTestSurface />
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
          <DuplicateTestSurface />
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
