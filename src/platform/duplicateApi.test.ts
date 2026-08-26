import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from './ipc';
import {
  getDuplicatePairs,
  resolveDuplicatePair,
  scanDuplicates,
  type DuplicatePair,
} from './duplicateApi';

vi.mock('./ipc', () => ({ invoke: vi.fn() }));

const candidate = (): Omit<DuplicatePair, 'status'> => ({
  file_id_a: 4,
  file_id_b: 9,
  distance: 8,
  left: {
    file: {
      file_id: 4,
      file_hash: 'left-hash',
      mime_type: 'image/png',
      size_bytes: 200,
      pixel_width: 100,
      pixel_height: 100,
      frame_count: null,
      decoded_information: null,
      has_alpha: null,
    },
    occurrences: [{ media_item_id: 41, root_item_id: 41, collection_id: null }],
  },
  right: {
    file: {
      file_id: 9,
      file_hash: 'right-hash',
      mime_type: 'image/jpeg',
      size_bytes: 100,
      pixel_width: 100,
      pixel_height: 100,
      frame_count: null,
      decoded_information: null,
      has_alpha: null,
    },
    occurrences: [{ media_item_id: 92, root_item_id: 92, collection_id: null }],
  },
  decision: 'AutoTieLeft',
});

describe('duplicateApi', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('uses replacement scan and list commands', async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({ candidate_count: 2, affected_item_ids: [41, 92], receipt: { revision: 3, resources: ['duplicates'], item_ids: [41, 92] } })
      .mockResolvedValueOnce([candidate()]);

    const scan = await scanDuplicates(7);
    const page = await getDuplicatePairs({ limit: 25 });

    expect(invoke).toHaveBeenNthCalledWith(1, 'duplicates.scan', { distance_threshold: 7 });
    expect(invoke).toHaveBeenNthCalledWith(2, 'duplicates.list', { limit: 25 });
    expect(scan.candidate_count).toBe(2);
    expect(page.items[0].left.occurrences).toEqual([
      { media_item_id: 41, root_item_id: 41, collection_id: null },
    ]);
    expect(page.total).toBe(1);
  });

  it('uses the broadened global candidate threshold by default', async () => {
    vi.mocked(invoke).mockResolvedValue({
      candidate_count: 0,
      affected_item_ids: [],
      receipt: { revision: 3, resources: ['duplicates'], item_ids: [] },
    });

    await scanDuplicates();

    expect(invoke).toHaveBeenCalledWith('duplicates.scan', { distance_threshold: 16 });
  });

  it('resolves explicit choices using physical file IDs', async () => {
    vi.mocked(invoke).mockResolvedValue({
      choice: { KeepFile: { winner_file_id: 4 } },
      affected_item_ids: [41, 92],
      freed_file_hash: 'right-hash',
      receipt: { revision: 4, resources: ['duplicates', 'library'], item_ids: [41, 92] },
    });
    const pair = { ...candidate(), status: 'detected' as const };

    const result = await resolveDuplicatePair('keep_left', pair);

    expect(invoke).toHaveBeenCalledWith('duplicates.resolve', {
      file_id_a: 4,
      file_id_b: 9,
      choice: { KeepFile: { winner_file_id: 4 } },
    });
    expect(result.status).toBe('resolved');
    expect(result.affected_item_ids).toEqual([41, 92]);
  });

  it('uses automatic resolution without sending renderer-only fields', async () => {
    vi.mocked(invoke).mockResolvedValue({
      choice: { KeepFile: { winner_file_id: 4 } },
      affected_item_ids: [41, 92],
      freed_file_hash: 'right-hash',
      receipt: { revision: 5, resources: ['duplicates', 'library'], item_ids: [41, 92] },
    });
    const pair = { ...candidate(), status: 'detected' as const };

    await resolveDuplicatePair('smart_merge', pair);

    expect(invoke).toHaveBeenCalledWith('duplicates.resolve_automatically', {
      file_id_a: 4,
      file_id_b: 9,
    });
  });

  it('lets the backend make the current quality decision', async () => {
    vi.mocked(invoke).mockResolvedValue(null);
    const pair = { ...candidate(), decision: 'NeedsChoice' as const, status: 'detected' as const };

    const result = await resolveDuplicatePair('smart_merge', pair);

    expect(invoke).toHaveBeenCalledWith('duplicates.resolve_automatically', {
      file_id_a: 4,
      file_id_b: 9,
    });
    expect(result.status).toBe('quality_ambiguous');
  });
});
