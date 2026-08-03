import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
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
      });

    const user = userEvent.setup();
    await act(async () => {
      render(<DuplicatesScreen />);
    });

    await screen.findByText('Left image');
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
});
