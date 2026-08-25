import { describe, expect, it, vi } from 'vitest';
import { buildCommonTagContextEntries, tagName } from './tagContextMenu';

const namespaces = [
  { namespace: 'general', count: 3 },
  { namespace: 'character', count: 2 },
  { namespace: 'creator', count: 1 },
];

function labels(entries: ReturnType<typeof buildCommonTagContextEntries>): string[] {
  return entries.flatMap((entry) => ('label' in entry ? [entry.label] : []));
}

describe('shared tag context actions', () => {
  it('treats general as ungrouped and offers existing namespaces', () => {
    const onMoveToGroup = vi.fn();
    const entries = buildCommonTagContextEntries({
      tag: { namespace: 'general', subtag: 'blue eyes' },
      namespaces,
      starred: false,
      onFilter: vi.fn(),
      onStarChange: vi.fn(),
      onMoveToGroup,
    });

    expect(tagName({ namespace: 'general', subtag: 'blue eyes' })).toBe('blue eyes');
    expect(labels(entries)).toContain('Add to Group…');
    expect(labels(entries)).not.toContain('Remove from this Group');
    const groupMenu = entries.find((entry) => 'submenu' in entry && entry.label === 'Add to Group…');
    expect(groupMenu && 'children' in groupMenu
      ? groupMenu.children.flatMap((entry) => ('label' in entry ? [entry.label] : []))
      : []).toEqual(['character', 'creator']);
  });

  it('uses the same core actions and moves grouped tags without another model', () => {
    const onMoveToGroup = vi.fn();
    const entries = buildCommonTagContextEntries({
      tag: { namespace: 'character', subtag: 'alice' },
      namespaces,
      starred: true,
      onFilter: vi.fn(),
      onStarChange: vi.fn(),
      onMoveToGroup,
      onRemove: vi.fn(),
    });

    expect(labels(entries)).toEqual(expect.arrayContaining([
      'Filter Items with This Tag',
      'Remove from Starred',
      'Move to Group…',
      'Remove from this Group',
      'Copy Tag',
      'Remove Tag',
    ]));
  });
});
