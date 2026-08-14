import { describe, expect, it, vi } from 'vitest';
import { getTagsPaginated } from './tagApi';
import { invoke } from './ipc';

vi.mock('./ipc', () => ({
  invoke: vi.fn(),
}));

describe('tagApi pagination', () => {
  it('returns the backend page and opaque cursor unchanged', async () => {
    const page = {
      items: [{ tag_id: 7, namespace: 'character', subtag: 'alice', file_count: 2 }],
      next_cursor: 'backend-cursor',
    };
    vi.mocked(invoke).mockResolvedValue(page);

    await expect(getTagsPaginated({ limit: 1 })).resolves.toEqual(page);
    expect(invoke).toHaveBeenCalledWith('get_tags_paginated', { limit: 1 });
  });
});
