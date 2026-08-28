import { getDefaultStore } from 'jotai';
import { queryItems } from '../../platform/entityApi';
import { initialGridFilters } from '../../state/grid';
import { compileGridQuery } from '../../shared/lib/itemFilters';
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
  cursor: string | null = null,
): Promise<MediaCoverCandidatePage<string>> {
  const library = (window as any).picto?.library;
  if (!library) throw new Error('Library service is unavailable.');
  const config = await library.getConfig();
  if (config.currentPath !== libraryPath) {
    throw new Error('Open this library before choosing one of its media items.');
  }
  const page = await queryItems(compileGridQuery(
    { kind: 'all' },
    initialGridFilters,
    { field: 'imported_at', direction: 'descending', random_seed: null },
  ), { cursor, limit: PAGE_SIZE });
  return {
    candidates: page.items
      .filter((item) => item.kind === 'media')
      .map((item) => ({
        media_item_id: item.root_id,
        file_hash: item.content_hash,
        name: item.name,
        pixel_width: item.width,
        pixel_height: item.height,
        mime_type: item.mime,
      })),
    next_cursor: page.next_cursor,
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
