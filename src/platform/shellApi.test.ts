import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from './ipc';
import { reverseImageSearch } from './shellApi';

vi.mock('./ipc', () => ({ invoke: vi.fn() }));

describe('reverseImageSearch', () => {
  const reverseImage = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('window', { picto: { search: { reverseImage } } });
  });

  it('resolves the original file and uploads it through the desktop search bridge', async () => {
    vi.mocked(invoke).mockResolvedValue([{ path: '/library/original.png' }]);

    await reverseImageSearch('file-hash', 'sogou');

    expect(invoke).toHaveBeenCalledWith('media.resolve_paths', { file_hashes: ['file-hash'] });
    expect(reverseImage).toHaveBeenCalledWith('/library/original.png', 'sogou');
  });

  it('fails clearly when the physical file cannot be resolved', async () => {
    vi.mocked(invoke).mockResolvedValue([]);

    await expect(reverseImageSearch('missing-hash', 'tineye'))
      .rejects.toThrow('Physical file not found: missing-hash');
    expect(reverseImage).not.toHaveBeenCalled();
  });
});
