import { useState, useEffect, useRef, useLayoutEffect, useCallback, useMemo } from 'react';
import { TextInput } from '@mantine/core';
import { IconSearch, IconCheck } from '@tabler/icons-react';
import { api } from '#desktop/api';
import { getNamespaceColor } from '../lib/namespaceColors';
import { registerTagPickerOpenHandler, type TagPickerRequest } from './tagPickerService';
import { OverlayShell } from '../components/OverlayShell';
import classes from '../components/ContextMenu.module.css';

interface TagEntry {
  display: string;
  namespace: string;
  count: number;
}

const PAGE_SIZE = 200;

export function TagPickerPortal() {
  const [request, setRequest] = useState<TagPickerRequest | null>(null);

  useEffect(() => {
    return registerTagPickerOpenHandler((req) => setRequest(req));
  }, []);

  const handleClose = useCallback(() => setRequest(null), []);

  if (!request) return null;

  return (
    <TagPickerPanel
      anchorEl={request.anchorEl}
      initialSelected={request.selected}
      onToggle={request.onToggle}
      onClose={handleClose}
    />
  );
}

function TagPickerPanel({
  anchorEl,
  initialSelected,
  onToggle,
  onClose,
}: {
  anchorEl: HTMLElement;
  initialSelected: string[];
  onToggle: (tag: string, added: boolean) => void;
  onClose: () => void;
}) {
  const menuRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [search, setSearch] = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [tags, setTags] = useState<TagEntry[]>([]);
  const [hasMore, setHasMore] = useState(true);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(() => new Set(initialSelected));
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const offsetRef = useRef(0);
  const itemsRef = useRef<HTMLDivElement>(null);

  // PBI-038: Debounce search input (150ms).
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 150);
    return () => clearTimeout(timer);
  }, [search]);

  // PBI-038: Fetch paged tags from backend.
  const fetchPage = useCallback(async (query: string, offset: number, append: boolean) => {
    setLoading(true);
    try {
      const tuples = await api.tags.searchPaged(query, PAGE_SIZE, offset);
      const entries: TagEntry[] = tuples.map(([display, namespace, count]) => ({
        display,
        namespace: namespace || '',
        count,
      }));
      setTags((prev) => append ? [...prev, ...entries] : entries);
      setHasMore(entries.length >= PAGE_SIZE);
      offsetRef.current = offset + entries.length;
    } catch (e) {
      console.error('Failed to fetch tags:', e);
    } finally {
      setLoading(false);
    }
  }, []);

  // Fetch initial page and reset on search change.
  useEffect(() => {
    offsetRef.current = 0;
    setTags([]);
    setHasMore(true);
    fetchPage(debouncedSearch, 0, false);
  }, [debouncedSearch, fetchPage]);

  // PBI-038: Load next page on scroll to bottom.
  const handleScroll = useCallback((e: React.UIEvent<HTMLDivElement>) => {
    if (!hasMore || loading) return;
    const el = e.currentTarget;
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 20) {
      fetchPage(debouncedSearch, offsetRef.current, true);
    }
  }, [hasMore, loading, debouncedSearch, fetchPage]);

  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const anchorRect = anchorEl.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();

    let x = anchorRect.left;
    let y = anchorRect.bottom + 4;

    if (x + elRect.width > window.innerWidth - 8) x = window.innerWidth - elRect.width - 8;
    if (y + elRect.height > window.innerHeight - 8) y = anchorRect.top - elRect.height - 4;
    if (x < 8) x = 8;
    if (y < 8) y = 8;

    setPos({ x, y });
  }, [anchorEl, tags.length]);

  useEffect(() => { searchRef.current?.focus(); }, []);

  const grouped = useMemo(() => {
    const m = new Map<string, TagEntry[]>();
    for (const tag of tags) {
      const ns = tag.namespace || '';
      if (!m.has(ns)) m.set(ns, []);
      m.get(ns)!.push(tag);
    }
    return m;
  }, [tags]);

  const toggleTag = useCallback((display: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      const added = !next.has(display);
      if (added) next.add(display); else next.delete(display);
      onToggle(display, added);
      return next;
    });
  }, [onToggle]);

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
            onChange={(e) => setSearch(e.currentTarget.value)}
            placeholder="Search tags..."
            leftSection={<IconSearch stroke={1.5} />}
            leftSectionWidth={21}
            variant="unstyled"
            size="xs"
            styles={{ input: { paddingLeft: 21, fontSize: 'var(--mantine-font-size-md)' } }}
          />
        </div>

        <div
          ref={itemsRef}
          className={classes.items}
          style={{ maxHeight: 340, overflowY: 'auto' }}
          onScroll={handleScroll}
        >
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
                const isChecked = selected.has(tag.display);
                return (
                  <div
                    key={tag.display}
                    className={classes.item}
                    onClick={() => toggleTag(tag.display)}
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
          {tags.length === 0 && !loading && (
            <div className={classes.empty}>No tags found</div>
          )}
        </div>
      </div>
    </OverlayShell>
  );
}
