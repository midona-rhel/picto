import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.hoisted(() => vi.fn());

vi.mock('./ipc', () => ({ invoke }));

import { getNavigation, getSidebarCounts } from './navigationApi';

describe('replacement navigation API', () => {
  beforeEach(() => invoke.mockReset());

  it('uses only replacement read commands', async () => {
    invoke.mockResolvedValueOnce({ folders: [], smart_folders: [], revision: 1 });
    invoke.mockResolvedValueOnce({
      all: 0,
      inbox: 0,
      trash: 0,
      recently_viewed: 0,
      untagged: 0,
      uncategorized: 0,
      duplicates: 0,
      folders: [],
      smart_folders: [],
      revision: 1,
    });

    await getNavigation();
    await getSidebarCounts();

    expect(invoke.mock.calls).toEqual([
      ['navigation.get', {}],
      ['sidebar.counts', {}],
    ]);
  });
});
