import { afterEach, describe, expect, it } from 'vitest';
import { setLocale } from '../i18n';
import { describeHistoryEntry } from './historyMessages';

const entry = (command: string, count = 0, detail: string | null = null) => ({
  entry_id: 1, command, label: 'developer label', count, detail,
});

describe('describeHistoryEntry', () => {
  afterEach(() => setLocale('en'));

  it('formats numbers using the selected application language', () => {
    setLocale('de');
    expect(describeHistoryEntry(entry('items.set_lifecycle', 1000, 'trash'))).toContain('1.000');
    setLocale('en');
    expect(describeHistoryEntry(entry('items.set_lifecycle', 1000, 'trash'))).toContain('1,000');
  });

  it('says what happened to the items in plain words', () => {
    expect(describeHistoryEntry(entry('items.set_lifecycle', 3, 'trash'))).toBe('Moved 3 items to Trash');
    expect(describeHistoryEntry(entry('items.set_lifecycle', 1, 'inbox'))).toBe('Moved an item to the Inbox');
    expect(describeHistoryEntry(entry('items.set_lifecycle', 2, 'active'))).toBe('Moved 2 items into the library');
    expect(describeHistoryEntry(entry('collections.detach', 1))).toBe('Removed an item from a collection');
    expect(describeHistoryEntry(entry('collections.organize', 4))).toBe('Grouped 4 items into a collection');
    expect(describeHistoryEntry(entry('items.set_rating', 5, '4'))).toBe('Rated 5 items 4 stars');
    expect(describeHistoryEntry(entry('items.set_rating', 1, '1'))).toBe('Rated an item 1 star');
    expect(describeHistoryEntry(entry('items.set_rating', 5, '1'))).toBe('Rated 5 items 1 star');
    expect(describeHistoryEntry(entry('items.set_rating', 1, '0'))).toBe('Cleared the rating of an item');
    expect(describeHistoryEntry(entry('items.apply_tags', 2, 'remove'))).toBe('Removed tags from 2 items');
    expect(describeHistoryEntry(entry('items.set_folder', 1, 'add'))).toBe('Added an item to a folder');
    expect(describeHistoryEntry(entry('tags.rename_or_merge', 0, 'merge'))).toBe('Merged tags');
  });

  it('falls back to the backend label for a command it does not know', () => {
    expect(describeHistoryEntry(entry('settings.something'))).toBe('developer label');
  });

  it.each([
    [1, 'Deleted a smart folder'],
    [3, 'Deleted 3 smart folders'],
  ])('describes deletion of %i smart folders', (count, expected) => {
    expect(describeHistoryEntry(entry('smart_folders.delete', count))).toBe(expected);
  });

  it('describes renderer metadata patches and watch edits accurately', () => {
    expect(describeHistoryEntry(entry('items.patch_metadata', 2, 'rating:4'))).toBe('Rated 2 items 4 stars');
    expect(describeHistoryEntry(entry('items.patch_metadata', 1, 'rating:1'))).toBe('Rated an item 1 star');
    expect(describeHistoryEntry(entry('items.patch_metadata', 1, 'rating:0'))).toBe('Cleared the rating of an item');
    expect(describeHistoryEntry(entry('items.patch_metadata', 1, 'notes'))).toBe('Changed notes');
    expect(describeHistoryEntry(entry('items.patch_metadata', 1, 'source_urls'))).toBe('Changed source links');
    expect(describeHistoryEntry(entry('items.patch_metadata', 1))).toBe('Changed the details of an item');
    expect(describeHistoryEntry(entry('folders.set_watch'))).toBe('Changed folder watching');
  });
});
