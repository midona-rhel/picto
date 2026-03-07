import { useState, useCallback, useRef } from 'react';
import { getShortcutGroups, formatKeysDisplay, SHORTCUT_DEFS } from '../../../shared/lib/shortcuts';
import { TextButton } from '../../../shared/components/TextButton';
import type { ShortcutDef, ShortcutGroup } from '../../../shared/lib/shortcuts';
import st from './ShortcutsPanel.module.css';
import settingsSt from './Settings.module.css';

const isMac =
  typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

// ---------------------------------------------------------------------------
// Key capture: converts a KeyboardEvent into a shortcut string like "Mod+Shift+Z"
// ---------------------------------------------------------------------------
function eventToShortcutString(e: React.KeyboardEvent): string | null {
  const key = e.key;
  // Ignore lone modifier presses
  if (['Control', 'Shift', 'Alt', 'Meta', 'OS'].includes(key)) return null;

  const parts: string[] = [];

  // Modifier order: Mod, Ctrl (if not Mod), Alt, Shift
  if (isMac ? e.metaKey : e.ctrlKey) parts.push('Mod');
  if (isMac && e.ctrlKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');

  // Normalize key names
  let normalized = key;
  if (key === ' ') normalized = 'Space';
  else if (key === '+') normalized = '+';
  else if (key === '-') normalized = '-';
  else if (key === '`') normalized = '`';
  else if (key.startsWith('Arrow')) normalized = key; // ArrowLeft, ArrowRight, etc.
  else if (/^F\d{1,2}$/.test(key)) normalized = key; // F1-F12
  else if (key.length === 1) normalized = key.toUpperCase();

  parts.push(normalized);
  return parts.join('+');
}

// ---------------------------------------------------------------------------
// ShortcutInput — captures keystrokes and displays platform-formatted shortcut
// ---------------------------------------------------------------------------
interface ShortcutInputProps {
  value: string;
  onChange: (keys: string) => void;
  conflict?: string | null; // conflicting shortcut label, if any
}

function ShortcutInput({ value, onChange, conflict }: ShortcutInputProps) {
  const [editing, setEditing] = useState(false);
  const [tempValue, setTempValue] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  const displayValue = editing ? (tempValue || 'Press shortcut…') : formatKeysDisplay(value);

  const handleFocus = useCallback(() => {
    setEditing(true);
    setTempValue('');
  }, []);

  const handleBlur = useCallback(() => {
    setEditing(false);
    setTempValue('');
  }, []);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      e.preventDefault();
      e.stopPropagation();

      const result = eventToShortcutString(e);
      if (result === null) return; // lone modifier, ignore

      setTempValue(formatKeysDisplay(result));
      onChange(result);

      // Auto-blur after capture
      setTimeout(() => {
        setEditing(false);
        inputRef.current?.blur();
      }, 150);
    },
    [onChange],
  );

  const handleClear = useCallback(
    (e: React.MouseEvent<HTMLButtonElement>) => {
      e.preventDefault();
      e.stopPropagation();
      onChange('');
      setTempValue('');
      setEditing(false);
      inputRef.current?.blur();
    },
    [onChange],
  );

  const cls = [
    st.shortcutInput,
    conflict ? st.shortcutConflict : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div className={st.shortcutInputWrap}>
      <input
        ref={inputRef}
        className={cls}
        value={displayValue}
        readOnly
        onFocus={handleFocus}
        onBlur={handleBlur}
        onKeyDown={editing ? handleKeyDown : undefined}
      />
      {value && (
        <button
          type="button"
          className={st.clearShortcutButton}
          onMouseDown={(e) => e.preventDefault()}
          onClick={handleClear}
          aria-label="Clear shortcut"
          title="Clear shortcut"
        >
          Clear
        </button>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// ShortcutsPanel
// ---------------------------------------------------------------------------
export function ShortcutsPanel() {
  const groups = getShortcutGroups();
  const [search, setSearch] = useState('');

  // Local overrides: id → keys. Empty string = cleared. undefined = default.
  const [overrides, setOverrides] = useState<Record<string, string>>({});
  // Secondary overrides: id → keys2
  const [overrides2, setOverrides2] = useState<Record<string, string>>({});
  // Conflict flash: id → conflicting label
  const [conflicts, setConflicts] = useState<Record<string, string>>({});

  const getEffectiveKeys = useCallback(
    (def: ShortcutDef) => overrides[def.id] ?? def.keys,
    [overrides],
  );

  const getEffectiveKeys2 = useCallback(
    (def: ShortcutDef) => overrides2[def.id] ?? def.keys2 ?? '',
    [overrides2],
  );

  // Filter groups by search
  const filteredGroups = filterGroups(groups, search);

  // Detect conflicts when overrides change
  const handleChange = useCallback(
    (id: string, newKeys: string) => {
      // Check for conflicts against all other shortcuts
      if (newKeys) {
        const conflicting = SHORTCUT_DEFS.find(
          (d) => d.id !== id && (overrides[d.id] ?? d.keys) === newKeys,
        );
        if (conflicting) {
          setConflicts((prev) => ({ ...prev, [id]: conflicting.label }));
          // Revert after 1.5s
          setTimeout(() => {
            setConflicts((prev) => {
              const next = { ...prev };
              delete next[id];
              return next;
            });
            setOverrides((prev) => {
              const next = { ...prev };
              delete next[id];
              return next;
            });
          }, 1500);
          // Still set it temporarily so user sees the conflict state
          setOverrides((prev) => ({ ...prev, [id]: newKeys }));
          return;
        }
      }
      setOverrides((prev) => ({ ...prev, [id]: newKeys }));
    },
    [overrides],
  );

  const handleChange2 = useCallback(
    (id: string, newKeys: string) => {
      setOverrides2((prev) => ({ ...prev, [id]: newKeys }));
    },
    [],
  );

  const handleRestoreDefaults = useCallback(() => {
    setOverrides({});
    setOverrides2({});
    setConflicts({});
  }, []);

  const hasOverrides = Object.keys(overrides).length > 0 || Object.keys(overrides2).length > 0;

  return (
    <div>
      <div className={settingsSt.panelBlock}>
        <div className={settingsSt.blockTitle}>Keyboard Shortcuts</div>
        <div className={settingsSt.blockContent} style={{ padding: 0, overflow: 'hidden' }}>
          {/* Search */}
          <input
            className={st.searchInput}
            placeholder="Search shortcuts…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />

          {/* Groups */}
          <div style={{ padding: '12px 12px 8px' }}>
            {filteredGroups.length === 0 && (
              <div className={st.emptyState}>No shortcuts match your search.</div>
            )}
            {filteredGroups.map((group) => (
              <div key={group.name}>
                <div className={st.groupTitle}>
                  {group.name}
                  <span className={st.groupCount}>({group.items.length})</span>
                </div>
                <div className={st.shortcutTable}>
                  {group.items.map((def) => (
                    <div key={def.id} className={st.tableRow}>
                      <div className={st.functionName}>
                        <div className={st.functionNameText}>{def.label}</div>
                        {def.description && (
                          <div className={st.functionDescription}>{def.description}</div>
                        )}
                      </div>
                      <div className={st.functionShortcut}>
                        <ShortcutInput
                          value={getEffectiveKeys(def)}
                          onChange={(keys) => handleChange(def.id, keys)}
                          conflict={conflicts[def.id] ?? null}
                        />
                      </div>
                      <div className={st.functionShortcut2}>
                        <ShortcutInput
                          value={getEffectiveKeys2(def)}
                          onChange={(keys) => handleChange2(def.id, keys)}
                        />
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Restore defaults */}
      {hasOverrides && (
        <div className={st.restoreDefaults}>
          <TextButton compact onClick={handleRestoreDefaults}>
            Restore Defaults
          </TextButton>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Filter helper
// ---------------------------------------------------------------------------
function filterGroups(groups: ShortcutGroup[], query: string): ShortcutGroup[] {
  if (!query.trim()) return groups;
  const q = query.toLowerCase();
  return groups
    .map((g) => ({
      ...g,
      items: g.items.filter(
        (d) =>
          d.label.toLowerCase().includes(q) ||
          d.group.toLowerCase().includes(q) ||
          (d.description?.toLowerCase().includes(q) ?? false) ||
          formatKeysDisplay(d.keys).toLowerCase().includes(q),
      ),
    }))
    .filter((g) => g.items.length > 0);
}
