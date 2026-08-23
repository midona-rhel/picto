import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { Notifications, notifications } from '@mantine/notifications';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  getDuplicateItemDetails,
  getDuplicatePairs,
  resolveDuplicatePair,
  scanDuplicates,
  type DuplicatePair,
} from '../../platform/duplicateApi';
import type { ItemDetails } from '../../shared/types/generated/application/ItemDetails';
import { DuplicatesScreen } from './DuplicatesScreen';

vi.mock('../../platform/duplicateApi', () => ({
  getDuplicateItemDetails: vi.fn(),
  getDuplicatePairs: vi.fn(),
  resolveDuplicatePair: vi.fn(),
  scanDuplicates: vi.fn(),
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
    distance: 5,
    left: { file: file(1, 'left'), item_ids: [11] },
    right: { file: file(2, 'right'), item_ids: [22] },
    decision,
    similarity_pct: 98.046875,
    status: 'detected',
  };
}

function details(itemId: number, hash: string, name: string): ItemDetails {
  return {
    item_id: itemId,
    kind: 'media',
    lifecycle: 'active',
    label: name,
    cover_media_item_id: null,
    folder_ids: [],
    media: [{
      media_item_id: itemId,
      file_hash: hash,
      mime_type: 'image/png',
      dominant_color_hex: null,
      size_bytes: 1_024,
      pixel_width: 100,
      pixel_height: 100,
      duration_ms: null,
      frame_count: null,
      has_audio: false,
      name,
      notes: null,
      rating: null,
      source_urls: [],
      captured_at: null,
      imported_at: '2026-01-01T00:00:00Z',
      position: 0,
      tags: [],
    }],
    aggregate_tags: [],
    revision: 1,
  };
}

function renderScreen(withNotifications = false) {
  return render(
    <MantineProvider forceColorScheme="dark">
      {withNotifications && <Notifications />}
      <DuplicatesScreen />
    </MantineProvider>,
  );
}

describe('DuplicatesScreen', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    notifications.clean();
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
    await act(async () => { renderScreen(); });

    expect(await screen.findByText('Left image')).toBeInTheDocument();
    expect(screen.getByText('Right image')).toBeInTheDocument();
    expect(getDuplicateItemDetails).toHaveBeenCalledWith(11);
    expect(getDuplicateItemDetails).toHaveBeenCalledWith(22);
    expect(screen.getByText('98% match')).toBeInTheDocument();
    expect(screen.getByTestId('left-preview-layers').querySelector('img')).toHaveAttribute(
      'src',
      'media://localhost/thumb/left.jpg',
    );
  });

  it('resolves a pair through the replacement candidate contract', async () => {
    const user = userEvent.setup();
    await act(async () => { renderScreen(); });
    await screen.findByText('Left image');

    await user.click(screen.getByRole('button', { name: 'Keep left' }));

    await waitFor(() => expect(resolveDuplicatePair).toHaveBeenLastCalledWith('keep_left', expect.objectContaining({
      file_id_a: 1,
      file_id_b: 2,
    })));
  });

  it('keeps an ambiguous smart merge in review for an explicit choice', async () => {
    const user = userEvent.setup();
    await act(async () => { renderScreen(true); });
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
    const user = userEvent.setup();
    await act(async () => { renderScreen(); });
    await screen.findByText('Left image');

    await user.click(screen.getByRole('button', { name: 'Zoom out' }));

    expect(screen.getByText('80%')).toBeInTheDocument();
    expect(screen.getByTestId('left-preview-layers').style.transform).toContain('scale(0.8)');
    expect(screen.getByTestId('right-preview-layers').style.transform).toContain('scale(0.8)');
  });

  it('shows the aligned difference composite only while the control is held', async () => {
    const user = userEvent.setup();
    await act(async () => { renderScreen(); });
    await screen.findByText('Left image');
    const control = screen.getByRole('button', { name: 'Highlight differences' });

    await user.hover(control);
    expect(screen.getByTestId('left-difference-composite')).toBeInTheDocument();
    expect(screen.getByTestId('right-difference-composite')).toBeInTheDocument();

    await user.unhover(control);
    expect(screen.queryByTestId('left-difference-composite')).not.toBeInTheDocument();
  });

  it('keeps decisions disabled while logical details are loading', async () => {
    vi.mocked(getDuplicateItemDetails).mockImplementation(() => new Promise(() => {}));
    await act(async () => { renderScreen(); });

    expect(await screen.findByRole('button', { name: 'Keep left' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Keep right' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Smart merge' })).toBeDisabled();
  });

  it('keeps the empty state stable and reports scan progress', async () => {
    vi.mocked(getDuplicatePairs).mockResolvedValue({ items: [], next_cursor: null, has_more: false, total: 0 });
    let finishScan!: (result: Awaited<ReturnType<typeof scanDuplicates>>) => void;
    vi.mocked(scanDuplicates).mockImplementationOnce(() => new Promise((resolve) => { finishScan = resolve; }));
    const user = userEvent.setup();
    await act(async () => { renderScreen(true); });
    await screen.findByRole('region', { name: 'No duplicate pairs' });

    await user.click(screen.getByRole('button', { name: 'Scan library' }));
    expect(screen.getByRole('region', { name: 'No duplicate pairs' })).toBeInTheDocument();
    expect(await screen.findByRole('progressbar')).toBeInTheDocument();

    finishScan({
      candidate_count: 0,
      affected_item_ids: [],
      receipt: { revision: 2, resources: ['duplicates'], item_ids: [] },
    });
    await waitFor(() => expect(screen.queryByRole('progressbar')).not.toBeInTheDocument());
    expect(await screen.findByRole('alert')).toHaveTextContent('Scan complete - no new review pairs');
  });
});
