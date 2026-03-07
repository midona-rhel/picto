import { useState, useEffect, useRef, useLayoutEffect, useCallback } from 'react';
import { TextInput } from '@mantine/core';
import { IconSearch, IconCheck } from '@tabler/icons-react';
import { api } from '#desktop/api';
import { getNamespaceColor } from '../../../shared/lib/namespaceColors';
import { OverlayShell } from '../../../shared/components/OverlayShell';
import classes from '../../../shared/components/ContextMenu.module.css';

interface TagPickerMenuProps {
  selected: string[];
  onChange: (tags: string[]) => void;
  anchorRef: React.RefObject<HTMLElement | null>;
  onClose: () => void;
}

interface TagEntry {
  display: string;
  namespace: string;
  count: number;
}

export function TagPickerMenu({ selected, onChange, anchorRef, onClose }: TagPickerMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [search, setSearch] = useState('');
  const [tags, setTags] = useState<TagEntry[]>([]);
  const [pos, setPos] = useState({ x: 0, y: 0 });

  // Fetch all tags with counts
  useEffect(() => {
    api.tags.getAll()
      .then((tuples) => {
        const entries: TagEntry[] = tuples.map(([display, namespace, count]) => ({
          display,
          namespace: namespace || '',
          count,
        }));
        entries.sort((a, b) => b.count - a.count);
        setTags(entries);
      })
      .catch((e) => console.error('Failed to fetch tags:', e));
  }, []);

  // Position below anchor
  useLayoutEffect(() => {
    const anchor = anchorRef.current;
    const el = menuRef.current;
    if (!anchor || !el) return;

    const anchorRect = anchor.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();

    let x = anchorRect.left;
    let y = anchorRect.bottom + 4;

    if (x + elRect.width > window.innerWidth - 8) x = window.innerWidth - elRect.width - 8;
    if (y + elRect.height > window.innerHeight - 8) y = anchorRect.top - elRect.height - 4;
    if (x < 8) x = 8;
    if (y < 8) y = 8;

    setPos({ x, y });
  }, [anchorRef, tags.length]);

  useEffect(() => {
    searchRef.current?.focus();
  }, []);

  const filtered = search
    ? tags.filter((t) => t.display.toLowerCase().includes(search.toLowerCase()))
    : tags;

  // Group by namespace
  const grouped = new Map<string, TagEntry[]>();
  for (const tag of filtered) {
    const ns = tag.namespace || '';
    if (!grouped.has(ns)) grouped.set(ns, []);
    grouped.get(ns)!.push(tag);
  }

  const toggleTag = useCallback((display: string) => {
    const set = new Set(selected);
    if (set.has(display)) {
      set.delete(display);
    } else {
      set.add(display);
    }
    onChange(Array.from(set));
  }, [selected, onChange]);

  const allItems: TagEntry[] = [];
  for (const [, entries] of grouped) {
    allItems.push(...entries);
  }

  return (
    <OverlayShell open onClose={onClose}>
      <div
        ref={menuRef}
        className={classes.panel}
        style={{ left: pos.x, top: pos.y, width: 300, maxHeight: 400 }}
        onContextMenu={(e) => e.preventDefault()}
      >
        <div className={classes.searchArea}>
          <TextInput
            ref={searchRef}
            value={search}
            onChange={(e) => { setSearch(e.currentTarget.value); }}
            placeholder="Search tags..."
            leftSection={<IconSearch stroke={1.5} />}
            leftSectionWidth={21}
            variant="unstyled"
            size="xs"
            styles={{ input: { paddingLeft: 21, fontSize: 'var(--mantine-font-size-md)' } }}
          />
        </div>

        <div className={classes.items} style={{ maxHeight: 340, overflowY: 'auto' }}>
          {Array.from(grouped.entries()).map(([ns, entries]) => (
            <div key={ns || '__unnamespaced'}>
              {ns && (
                <div style={{
                  padding: '4px 8px 2px',
                  fontSize: 'var(--mantine-font-size-xs)',
                  color: `rgb(${getNamespaceColor(ns, true).join(',')})`,
                  fontWeight: 600,
                  textTransform: 'uppercase',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 4,
                }}>
                  <span style={{
                    width: 6,
                    height: 6,
                    borderRadius: '50%',
                    background: `rgb(${getNamespaceColor(ns, true).join(',')})`,
                    flexShrink: 0,
                  }} />
                  {ns}
                </div>
              )}
              {entries.map((tag) => {
                const isChecked = selected.includes(tag.display);
                return (
                  <div
                    key={tag.display}
                    className={classes.item}
                    onClick={() => toggleTag(tag.display)}
                    onMouseEnter={() => {}}
                  >
                    <span className={classes.iconSlot}>
                      {isChecked && <IconCheck strokeWidth={2.5} />}
                    </span>
                    <span className={classes.label}>
                      {tag.namespace ? tag.display.split(':').slice(1).join(':') : tag.display}
                    </span>
                    <span className={classes.shortcut}>{tag.count}</span>
                  </div>
                );
              })}
            </div>
          ))}
          {filtered.length === 0 && (
            <div className={classes.empty}>No tags found</div>
          )}
        </div>
      </div>
    </OverlayShell>
  );
}
