import type { CanonicalEntityGridItem, SidebarNodeDto } from '../../../shared/types/canonical';

export interface CanvasRenderItem {
  hash: string;
  thumbnailHash: string;
  kind: 'single' | 'folder';
  name: string | null;
  mime: string;
  width: number | null;
  height: number | null;
  rating: number | null;
  durationMs: number | null;
  dominantColor: string | null;
  aspectRatio: number | null;
  numFrames: number | null;
  // Folder-specific fields
  folderIcon?: string | null;
  folderColor?: string | null;
  itemCount?: number | null;
  folderId?: number | null;
}

export type GridItem =
  | { kind: 'entity'; data: CanonicalEntityGridItem }
  | { kind: 'folder'; data: SidebarNodeDto; coverHash: string | null };

export function getItemHash(item: GridItem): string {
  return item.kind === 'entity' ? item.data.entity_hash : item.data.id;
}

export function adaptGridItem(item: CanonicalEntityGridItem): CanvasRenderItem {
  const aspectRatio = item.pixel_width && item.pixel_height
    ? item.pixel_width / item.pixel_height
    : null;

  return {
    hash: item.entity_hash,
    thumbnailHash: item.entity_hash,
    kind: 'single',
    name: item.name,
    mime: item.mime_type,
    width: item.pixel_width,
    height: item.pixel_height,
    rating: item.rating,
    durationMs: item.duration_ms,
    dominantColor: item.dominant_color_hex,
    aspectRatio,
    numFrames: item.frame_count,
  };
}

export function adaptFolderItem(folder: SidebarNodeDto, coverHash: string | null): CanvasRenderItem {
  const numericId = parseInt(folder.id.replace('folder:', ''), 10);
  return {
    hash: folder.id,
    thumbnailHash: coverHash ?? '',
    kind: 'folder',
    name: folder.name,
    mime: 'application/x-folder',
    width: null,
    height: null,
    rating: null,
    durationMs: null,
    dominantColor: null,
    aspectRatio: 4 / 3,
    numFrames: null,
    folderIcon: folder.icon ?? null,
    folderColor: folder.color ?? null,
    itemCount: folder.count,
    folderId: isNaN(numericId) ? null : numericId,
  };
}
