import { getDefaultStore } from 'jotai';
import { queryItems } from '../../platform/entityApi';
import { initialGridFilters } from '../../state/grid';
import { libraryCoverModalAtom } from '../../state/modals';
import type { MediaCoverCandidate, MediaCoverCandidatePage } from '../subscriptions/components/SubscriptionCoverDialog';

export interface LibraryCoverCrop {
  focusX: number;
  focusY: number;
  zoomPercent: number;
}

const PAGE_SIZE = 200;

export async function openCurrentLibraryCoverPicker(
  initialCandidate: MediaCoverCandidate | null = null,
): Promise<void> {
  const library = (window as any).picto?.library;
  if (!library) throw new Error('Library service is unavailable.');
  const config = await library.getConfig();
  if (!config.currentPath) throw new Error('No library is open.');
  const path = config.currentPath as string;
  const name = path.split(/[\\/]/).filter(Boolean).pop()?.replace(/\.library$/, '') ?? 'Library';
  getDefaultStore().set(libraryCoverModalAtom, {
    open: true,
    path,
    name,
    initialCandidate,
  });
}

export async function loadLibraryCoverCandidates(
  libraryPath: string,
  offset = 0,
): Promise<MediaCoverCandidatePage<number>> {
  const library = (window as any).picto?.library;
  if (!library) throw new Error('Library service is unavailable.');
  const config = await library.getConfig();
  if (config.currentPath !== libraryPath) {
    throw new Error('Open this library before choosing one of its media items.');
  }
  const page = await queryItems({
    scope: { kind: 'all' },
    filters: {
      ...initialGridFilters,
      include_tags: [],
      exclude_tags: [],
      include_folder_ids: [],
      exclude_folder_ids: [],
    },
    sort: { field: 'imported_at', direction: 'descending', random_seed: null },
  }, { offset, limit: PAGE_SIZE });
  return {
    candidates: page.items
      .filter((item) => item.kind === 'media')
      .map((item) => ({
        media_item_id: item.item_id,
        file_hash: item.display_file_hash,
        name: item.name,
        pixel_width: item.pixel_width,
        pixel_height: item.pixel_height,
        mime_type: item.display_mime_type,
      })),
    next_cursor: offset + page.items.length < (page.visible_item_count ?? 0)
      ? offset + page.items.length
      : null,
  };
}

export async function saveLibraryCover(
  libraryPath: string,
  candidate: MediaCoverCandidate,
  crop: LibraryCoverCrop,
): Promise<void> {
  const library = (window as any).picto?.library;
  if (!library) throw new Error('Library service is unavailable.');
  await library.setMeta(libraryPath, {
    imageHash: candidate.file_hash,
    imageFocusX: crop.focusX,
    imageFocusY: crop.focusY,
    imageZoomPercent: crop.zoomPercent,
    icon: null,
  });
}
