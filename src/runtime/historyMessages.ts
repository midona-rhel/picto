/**
 * Plain-language descriptions of undoable changes, for the Undo and Redo
 * notifications: "Moved 3 items to Trash", not the command behind it.
 */
import { t } from '../i18n';
import type { HistoryEntrySummary } from '../platform/historyApi';

type Describe = (count: number, detail: string | null) => string;

const DESCRIPTIONS: Record<string, Describe> = {
  'items.set_lifecycle': (count, detail) => {
    if (detail === 'trash') return (count === 1 ? t('Moved an item to Trash') : t('Moved {value0} items to Trash', { value0: count.toLocaleString() }));
    if (detail === 'inbox') return (count === 1 ? t('Moved an item to the Inbox') : t('Moved {value0} items to the Inbox', { value0: count.toLocaleString() }));
    return (count === 1 ? t('Moved an item into the library') : t('Moved {value0} items into the library', { value0: count.toLocaleString() }));
  },
  'items.set_rating': (count, detail) => {
    const stars = Number(detail ?? 0);
    if (!stars) return (count === 1 ? t('Cleared the rating of an item') : t('Cleared the rating of {value0} items', { value0: count.toLocaleString() }));
    if (stars === 1) return count === 1
      ? t('Rated an item 1 star')
      : t('Rated {value0} items 1 star', { value0: count.toLocaleString() });
    return count === 1
      ? t('Rated an item {value0} stars', { value0: stars })
      : t('Rated {value0} items {value1} stars', { value0: count.toLocaleString(), value1: stars });
  },
  'items.apply_tags': (count, detail) => (detail === 'remove'
    ? (count === 1 ? t('Removed tags from an item') : t('Removed tags from {value0} items', { value0: count.toLocaleString() }))
    : (count === 1 ? t('Added tags to an item') : t('Added tags to {value0} items', { value0: count.toLocaleString() }))),
  'items.apply_ai_tags': () => t('Added AI tags'),
  'items.set_folder': (count, detail) => (detail === 'remove'
    ? (count === 1 ? t('Removed an item from a folder') : t('Removed {value0} items from a folder', { value0: count.toLocaleString() }))
    : (count === 1 ? t('Added an item to a folder') : t('Added {value0} items to a folder', { value0: count.toLocaleString() }))),
  'items.patch_metadata': (count, detail) => {
    if (detail?.startsWith('rating:')) return DESCRIPTIONS['items.set_rating'](count, detail.slice(7));
    if (detail === 'notes') return t('Changed notes');
    if (detail === 'source_urls') return t('Changed source links');
    return count === 1 ? t('Changed the details of an item') : t('Changed the details of {value0} items', { value0: count.toLocaleString() });
  },
  'items.set_notes': () => t('Changed notes'),
  'items.set_source_urls': () => t('Changed source links'),
  'items.rename': () => t('Renamed an item'),
  'items.rename_many': (count) => (count === 1 ? t('Renamed an item') : t('Renamed {value0} items', { value0: count.toLocaleString() })),
  'items.clear_recent_views': () => t('Cleared recently viewed'),
  'collections.organize': (count) => (count === 1 ? t('Made a collection') : t('Grouped {value0} items into a collection', { value0: count.toLocaleString() })),
  'collections.ungroup': () => t('Ungrouped a collection'),
  'collections.detach': (count) => (count === 1 ? t('Removed an item from a collection') : t('Removed {value0} items from a collection', { value0: count.toLocaleString() })),
  'collections.reorder': () => t('Reordered a collection'),
  'collections.set_cover': () => t('Changed a collection’s cover'),
  'duplicates.resolve': () => t('Resolved a duplicate'),
  'folders.create': () => t('Created a folder'),
  'folders.duplicate': () => t('Duplicated a folder'),
  'folders.set_metadata': () => t('Edited a folder'),
  'folders.set_cover': () => t('Changed a folder’s cover'),
  'folders.set_watch': () => t('Changed folder watching'),
  'folders.clear_watch': () => t('Stopped watching a folder'),
  'folders.rename': () => t('Renamed a folder'),
  'folders.move': () => t('Moved a folder'),
  'folders.reorder': () => t('Reordered folders'),
  'folders.sort_tree': () => t('Sorted folders'),
  'folders.sort_items': () => t('Reordered items in a folder'),
  'folders.set_auto_tags': () => t('Changed a folder’s automatic tags'),
  'folders.delete': (count) => (count === 1 ? t('Deleted a folder') : t('Deleted {value0} folders', { value0: count.toLocaleString() })),
  'smart_folders.create': () => t('Created a smart folder'),
  'smart_folders.update': () => t('Edited a smart folder'),
  'smart_folders.delete': (count) => (count === 1 ? t('Deleted a smart folder') : t('Deleted {value0} smart folders', { value0: count.toLocaleString() })),
  'smart_folders.reorder': () => t('Reordered smart folders'),
  'tags.rename_or_merge': (_count, detail) => (detail === 'merge' ? t('Merged tags') : t('Renamed a tag')),
  'tags.delete': () => t('Deleted a tag'),
  'tags.delete_unused': () => t('Deleted unused tags'),
  'tags.group.create': () => t('Created a tag group'),
  'tags.group.rename': (_count, detail) => (detail === 'merge' ? t('Merged tag groups') : t('Renamed a tag group')),
};

/** The change in words; falls back to the backend label for an unknown command. */
export function describeHistoryEntry(entry: HistoryEntrySummary): string {
  const describe = DESCRIPTIONS[entry.command];
  return describe ? describe(entry.count ?? 0, entry.detail ?? null) : entry.label;
}
