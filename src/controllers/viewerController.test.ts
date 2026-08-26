import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ItemDetails } from '../shared/types/generated/application/ItemDetails';

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock('../platform/ipc', () => ({ invoke: invokeMock }));

import { viewerController } from './viewerController';

describe('viewerController group prefetch', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('keeps prefetched details available through Strict Mode render replay', async () => {
    const details = { kind: 'collection', item_id: 731 } as unknown as ItemDetails;
    invokeMock.mockResolvedValue(details);

    await viewerController.prefetchItemDetails(731);

    expect(viewerController.takePrefetchedItemDetails(731)).toBe(details);
    expect(viewerController.takePrefetchedItemDetails(731)).toBe(details);

    await Promise.resolve();
    expect(viewerController.takePrefetchedItemDetails(731)).toBeNull();
  });
});
