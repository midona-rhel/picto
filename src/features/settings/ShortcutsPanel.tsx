/**
 * ShortcutsPanel — displays all keyboard shortcuts grouped, with search.
 * Ported from legacy v0.5.0-alpha ShortcutsPanel.
 */

import { useState, useCallback, useRef, useMemo } from 'react';
import { getShortcutGroups, formatKeysDisplay, SHORTCUT_DEFS } from '../../shared/lib/shortcuts';
import type { ShortcutDef, ShortcutGroup } from '../../shared/lib/shortcuts';
import styles from './ShortcutsPanel.module.css';

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

function eventToShortcutString(e: React.KeyboardEvent): string | null {
  const key = e.key;
  if (['Control', 'Shift', 'Alt', 'Meta', 'OS'].includes(key)) return null;
  const parts: string[] = [];
  if (isMac ? e.metaKey : e.ctrlKey) parts.push('Mod');
  if (isMac && e.ctrlKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');
  let normalized = key;
  if (key === ' ') normalized = 'Space';
  else if (key === '+') normalized = '+';
  else if (key === '-') normalized = '-';
  else if (key === '`') normalized = '`';
  else if (key.startsWith('Arrow')) normalized = key;
  else if (/^F\d{1,2}$/.test(key)) normalized = key;
  else if (key.length === 1) normalized = key.toUpperCase();
  parts.push(normalized);
  return parts.join('+');
}

function ShortcutInput({ value, onChange, conflict }: { value: string; onChange: (k: string) => void; conflict?: string | null }) {
  const [editing, setEditing] = useState(false);
  const [temp, setTemp] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);
  const display = editing ? (temp || 'Press keys') : formatKeysDisplay(value);

  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLInputElement>) => {
    e.preventDefault(); e.stopPropagation();
    const result = eventToShortcutString(e);
    if (!result) return;
    setTemp(formatKeysDisplay(result));
    onChange(result);
    setTimeout(() => { setEditing(false); inputRef.current?.blur(); }, 150);
  }, [onChange]);

  return (
    <div className={styles.shortcutInputWrap}>
      <input
        ref={inputRef}
        className={`${styles.shortcutInput} ${conflict ? styles.shortcutConflict : ''} ${!value && !editing ? styles.shortcutEmpty : ''}`}
        value={display}
        placeholder="Click to bind"
        readOnly
        onFocus={() => { setEditing(true); setTemp(''); }}
        onBlur={() => { setEditing(false); setTemp(''); }}
        onKeyDown={editing ? handleKeyDown : undefined}
      />
      {value && (
        <button type="button" className={styles.clearBtn}
          onMouseDown={(e) => e.preventDefault()}
          onClick={(e) => { e.preventDefault(); onChange(''); setEditing(false); inputRef.current?.blur(); }}>
          ×
        </button>
      )}
    </div>
  );
}

function filterGroups(groups: ShortcutGroup[], query: string): ShortcutGroup[] {
  if (!query.trim()) return groups;
  const q = query.toLowerCase();
  return groups
    .map((g) => ({ ...g, items: g.items.filter((d) =>
      d.label.toLowerCase().includes(q) || d.group.toLowerCase().includes(q) ||
      (d.description?.toLowerCase().includes(q) ?? false) || formatKeysDisplay(d.keys).toLowerCase().includes(q),
    ) }))
    .filter((g) => g.items.length > 0);
}

export function ShortcutsPanel() {
  const groups = useMemo(() => getShortcutGroups(), []);
  const [search, setSearch] = useState('');
  const [overrides, setOverrides] = useState<Record<string, string>>({});
  const [overrides2, setOverrides2] = useState<Record<string, string>>({});
  const [conflicts, setConflicts] = useState<Record<string, string>>({});

  const getKeys = useCallback((def: ShortcutDef) => overrides[def.id] ?? def.keys, [overrides]);
  const getKeys2 = useCallback((def: ShortcutDef) => overrides2[def.id] ?? def.keys2 ?? '', [overrides2]);
  const filtered = useMemo(() => filterGroups(groups, search), [groups, search]);

  const handleChange = useCallback((id: string, newKeys: string) => {
    if (newKeys) {
      const conflicting = SHORTCUT_DEFS.find((d) => d.id !== id && (overrides[d.id] ?? d.keys) === newKeys);
      if (conflicting) {
        setConflicts((p) => ({ ...p, [id]: conflicting.label }));
        setTimeout(() => {
          setConflicts((p) => { const n = { ...p }; delete n[id]; return n; });
          setOverrides((p) => { const n = { ...p }; delete n[id]; return n; });
        }, 1500);
        setOverrides((p) => ({ ...p, [id]: newKeys }));
        return;
      }
    }
    setOverrides((p) => ({ ...p, [id]: newKeys }));
  }, [overrides]);

  const handleChange2 = useCallback((id: string, newKeys: string) => {
    setOverrides2((p) => ({ ...p, [id]: newKeys }));
  }, []);

  return (
    <div className={styles.root}>
      <input
        className={styles.searchInput}
        placeholder="Search shortcuts…"
        value={search}
        onChange={(e) => setSearch(e.target.value)}
      />
      <div className={styles.body}>
        {filtered.length === 0 && <div className={styles.empty}>No shortcuts match your search.</div>}
        {filtered.map((group) => (
          <div key={group.name}>
            <div className={styles.groupTitle}>{group.name} <span className={styles.groupCount}>({group.items.length})</span></div>
            <div className={styles.table}>
              {group.items.map((def) => (
                <div key={def.id} className={styles.row}>
                  <div className={styles.label}>
                    <div className={styles.labelText}>{def.label}</div>
                    {def.description && <div className={styles.labelDesc}>{def.description}</div>}
                  </div>
                  <div className={styles.binding}>
                    <ShortcutInput value={getKeys(def)} onChange={(k) => handleChange(def.id, k)} conflict={conflicts[def.id] ?? null} />
                  </div>
                  <div className={styles.binding}>
                    <ShortcutInput value={getKeys2(def)} onChange={(k) => handleChange2(def.id, k)} />
                  </div>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
