import type { ItemDetails } from '../types/generated/application/ItemDetails';

/** Tags present on every member; adding any other tag must fan out to all members. */
export function commonItemTags(details: ItemDetails | null): Set<string> {
  const [first, ...rest] = details?.media ?? [];
  if (!first) return new Set();
  const common = new Set(first.tags);
  for (const media of rest) {
    const tags = new Set(media.tags);
    for (const tag of common) {
      if (!tags.has(tag)) common.delete(tag);
    }
  }
  return common;
}
