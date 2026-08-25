import { afterEach, describe, expect, it, vi } from 'vitest';
import { setCurrentLibraryCover } from './libraryAppearance';

describe('setCurrentLibraryCover', () => {
  afterEach(() => { vi.unstubAllGlobals(); });

  it('updates only the current library and replaces its custom icon', async () => {
    const setMeta = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('window', {
      picto: { library: { getConfig: vi.fn().mockResolvedValue({ currentPath: '/Pictures/Test.library' }), setMeta } },
    });

    await setCurrentLibraryCover('media-hash');

    expect(setMeta).toHaveBeenCalledWith('/Pictures/Test.library', {
      imageHash: 'media-hash',
      icon: null,
    });
  });
});
