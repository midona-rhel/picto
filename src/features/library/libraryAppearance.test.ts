import { afterEach, describe, expect, it, vi } from 'vitest';
import { setCurrentLibraryImageIcon } from './libraryAppearance';

describe('setCurrentLibraryImageIcon', () => {
  afterEach(() => { vi.unstubAllGlobals(); });

  it('updates only the current library and replaces its custom icon', async () => {
    const setMeta = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('window', {
      picto: { library: { getConfig: vi.fn().mockResolvedValue({ currentPath: '/Pictures/Test.library' }), setMeta } },
    });

    await setCurrentLibraryImageIcon('image-hash');

    expect(setMeta).toHaveBeenCalledWith('/Pictures/Test.library', {
      imageHash: 'image-hash',
      icon: null,
    });
  });
});
