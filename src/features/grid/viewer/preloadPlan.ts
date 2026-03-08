import type { MasonryImageItem } from '../shared';

export type DecodePriority = 'high' | 'normal';

export interface NeighborThumbTask {
  hash: string;
  priority: DecodePriority;
}

export interface NeighborFullTask {
  hash: string;
  mime: string;
  priority: DecodePriority;
}

export interface NeighborDecodePlan {
  thumbs: NeighborThumbTask[];
  fulls: NeighborFullTask[];
}

const THUMB_OFFSETS = [-1, 1, -2, 2, -3, 3, -4, 4] as const;
const FULL_OFFSETS = [-1, 1, -2, 2, -3, 3] as const;

function toPriority(offset: number): DecodePriority {
  return Math.abs(offset) <= 1 ? 'high' : 'normal';
}

function isHeavyMime(mime: string): boolean {
  return mime === 'image/webp' || mime === 'image/avif';
}

export function buildNeighborDecodePlan(
  images: Array<Pick<MasonryImageItem, 'hash' | 'mime'>>,
  currentIndex: number,
  heavyDecodeLimit = 2,
): NeighborDecodePlan {
  const thumbs: NeighborThumbTask[] = [];
  const fulls: NeighborFullTask[] = [];
  let heavyQueued = 0;

  for (const offset of THUMB_OFFSETS) {
    const neighbor = images[currentIndex + offset];
    if (!neighbor) continue;
    thumbs.push({
      hash: neighbor.hash,
      priority: toPriority(offset),
    });
  }

  for (const offset of FULL_OFFSETS) {
    const neighbor = images[currentIndex + offset];
    if (!neighbor || neighbor.mime.startsWith('video/')) continue;
    if (isHeavyMime(neighbor.mime)) {
      if (heavyQueued >= heavyDecodeLimit) continue;
      heavyQueued += 1;
    }
    fulls.push({
      hash: neighbor.hash,
      mime: neighbor.mime,
      priority: toPriority(offset),
    });
  }

  return { thumbs, fulls };
}
