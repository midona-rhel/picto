import { getDefaultStore } from 'jotai';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { libraryCoverModalAtom } from '../../state/modals';
import { openCurrentLibraryCoverPicker, saveLibraryCover } from './libraryAppearance';

describe('library cover workflow', () => {
  afterEach(() => { vi.unstubAllGlobals(); });

  it('opens the crop workflow for the current library without writing metadata', async () => {
    const setMeta = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('window', {
      picto: { library: { getConfig: vi.fn().mockResolvedValue({ currentPath: '/Pictures/Test.library' }), setMeta } },
    });

    await openCurrentLibraryCoverPicker({
      media_item_id: 3,
      file_hash: 'media-hash',
      name: 'Cover',
      pixel_width: 1200,
      pixel_height: 800,
      mime_type: 'image/jpeg',
    });

    expect(setMeta).not.toHaveBeenCalled();
    expect(getDefaultStore().get(libraryCoverModalAtom)).toMatchObject({
      open: true,
      path: '/Pictures/Test.library',
      name: 'Test',
      initialCandidate: { file_hash: 'media-hash' },
    });
  });

  it('persists the chosen hash and exact normalized crop', async () => {
    const setMeta = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('window', { picto: { library: { setMeta } } });

    await saveLibraryCover('/Pictures/Test.library', {
      media_item_id: 3,
      file_hash: 'media-hash',
      name: 'Cover',
      pixel_width: 1200,
      pixel_height: 800,
    }, { focusX: 320, focusY: 610, zoomPercent: 145 });

    expect(setMeta).toHaveBeenCalledWith('/Pictures/Test.library', {
      imageHash: 'media-hash',
      imageFocusX: 320,
      imageFocusY: 610,
      imageZoomPercent: 145,
      icon: null,
    });
  });
});
