/**
 * Settings window — data-driven settings registry.
 *
 * Every setting is a row with { id, label, keywords, panel, render }.
 * Normal mode: sidebar nav selects a panel, content shows that panel's rows.
 * Search mode (>2 chars): filters ALL rows, groups results by panel name.
 * Footer always visible: Reset (revert to defaults) + Save (commit changes).
 */

import { useState, useMemo, useEffect, useCallback, type ReactNode } from 'react';
import { IconSettings2, IconCommand, IconX, IconSearch } from '@tabler/icons-react';
import { getKeyboardPreset, setKeyboardPreset, type KeyboardPreset } from '../../shared/lib/shortcuts';
import { ShortcutsPanel } from './ShortcutsPanel';
import styles from './Settings.module.css';

// ── Settings row definition ──

interface SettingRow {
  id: string;
  label: string;
  keywords: string;
  panel: string;
  render: (onDirty: () => void) => ReactNode;
}

// ── Panel definitions ──

interface PanelDef {
  id: string;
  label: string;
  icon: typeof IconSettings2;
  /** If set, renders a custom component instead of rows */
  custom?: (onDirty: () => void) => ReactNode;
}

const PANELS: PanelDef[] = [
  { id: 'general', label: 'General', icon: IconSettings2 },
  { id: 'shortcuts', label: 'Shortcuts', icon: IconCommand, custom: () => <ShortcutsPanel /> },
];

// ── Individual setting rows (for General + future panels) ──

function KeyboardPresetRow({ onDirty }: { onDirty: () => void }) {
  const [preset, setPreset] = useState<KeyboardPreset>(getKeyboardPreset());
  return (
    <div className={styles.settingRow}>
      <label className={styles.settingLabel}>Keyboard Layout</label>
      <div className={styles.settingControl}>
        <select
          className={styles.select}
          value={preset}
          onChange={(e) => {
            const v = e.target.value as KeyboardPreset;
            setPreset(v);
            setKeyboardPreset(v);
            onDirty();
          }}
        >
          <option value="us">US (QWERTY)</option>
          <option value="eu">EU (QWERTZ / AZERTY / Nordic)</option>
        </select>
        <p className={styles.settingHint}>
          EU mode swaps shortcuts that use backtick, backslash, and brackets for alternatives accessible on European keyboards.
        </p>
      </div>
    </div>
  );
}

function ThemePlaceholderRow() {
  return (
    <div className={styles.settingRow}>
      <label className={styles.settingLabel}>Theme</label>
      <div className={styles.settingControl}>
        <span className={styles.settingPlaceholder}>Theme selection coming soon.</span>
      </div>
    </div>
  );
}

const ALL_SETTINGS: SettingRow[] = [
  {
    id: 'general.keyboard',
    label: 'Keyboard Layout',
    keywords: 'keyboard layout qwerty qwertz azerty eu us european preset shortcut',
    panel: 'general',
    render: (onDirty) => <KeyboardPresetRow onDirty={onDirty} />,
  },
  {
    id: 'general.theme',
    label: 'Theme',
    keywords: 'theme appearance dark light color scheme',
    panel: 'general',
    render: () => <ThemePlaceholderRow />,
  },
];

// ── Component ──

export function Settings() {
  const [selected, setSelected] = useState('general');
  const [search, setSearch] = useState('');
  const [isDirty, setIsDirty] = useState(false);

  const markDirty = useCallback(() => setIsDirty(true), []);

  const activePanel = PANELS.find((p) => p.id === selected) ?? PANELS[0];
  const isSearching = search.trim().length > 2;

  // Filter settings by search query
  const searchResults = useMemo(() => {
    if (!isSearching) return [];
    const q = search.toLowerCase();
    return ALL_SETTINGS.filter((s) =>
      s.label.toLowerCase().includes(q) || s.keywords.toLowerCase().includes(q),
    );
  }, [search, isSearching]);

  // Group search results by panel
  const groupedResults = useMemo(() => {
    const map = new Map<string, SettingRow[]>();
    for (const row of searchResults) {
      let list = map.get(row.panel);
      if (!list) { list = []; map.set(row.panel, list); }
      list.push(row);
    }
    // Also include shortcut panel if query matches
    const q = search.toLowerCase();
    if ('keyboard shortcut keybind hotkey binding'.includes(q) || 'shortcuts'.includes(q)) {
      if (!map.has('shortcuts')) map.set('shortcuts', []);
    }
    return map;
  }, [searchResults, search]);

  // Filter sidebar nav
  const filteredPanels = useMemo(() => {
    if (!isSearching) return PANELS;
    const q = search.toLowerCase();
    return PANELS.filter((p) =>
      p.label.toLowerCase().includes(q) ||
      ALL_SETTINGS.some((s) => s.panel === p.id && (s.label.toLowerCase().includes(q) || s.keywords.toLowerCase().includes(q))),
    );
  }, [search, isSearching]);

  // Close confirmation
  useEffect(() => {
    if (!isDirty) return;
    const handler = (e: BeforeUnloadEvent) => { e.preventDefault(); e.returnValue = ''; };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [isDirty]);

  const handleClose = () => {
    if (isDirty && !window.confirm('You have unsaved changes. Close anyway?')) return;
    (window as any).picto?.api?.invoke('close_current_window')?.catch(() => { window.close(); });
  };

  const handleSave = () => setIsDirty(false);
  const handleReset = () => {
    if (!window.confirm('Reset all settings to defaults?')) return;
    setKeyboardPreset('us');
    setIsDirty(false);
    // Force re-render
    setSelected((s) => s);
  };

  // Rows for the currently selected panel
  const panelRows = ALL_SETTINGS.filter((s) => s.panel === selected);

  return (
    <div className={styles.root}>
      {/* ── Sidebar ── */}
      <div className={styles.sidebar}>
        <div className={styles.sidebarTitle}>Settings</div>
        <div className={styles.searchWrap}>
          <IconSearch size={14} className={styles.searchIcon} />
          <input
            className={styles.sidebarSearch}
            type="search"
            placeholder="Search settings..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <div className={styles.sidebarItems}>
          {filteredPanels.map((panel) => {
            const Icon = panel.icon;
            const isActive = panel.id === selected && !isSearching;
            return (
              <button
                key={panel.id}
                className={isActive ? styles.navItemActive : styles.navItem}
                onClick={() => { setSelected(panel.id); setSearch(''); }}
              >
                <Icon size={18} />
                {panel.label}
              </button>
            );
          })}
        </div>
      </div>

      {/* ── Content ── */}
      <div className={styles.content}>
        <div className={styles.contentHeader}>
          <span className={styles.contentTitle}>
            {isSearching ? `Results for "${search}"` : activePanel.label}
          </span>
          <button className={styles.closeBtn} onClick={handleClose}><IconX size={14} /></button>
        </div>

        <div className={styles.contentBody}>
          {isSearching ? (
            // Dynamic search view — matching rows grouped by panel
            groupedResults.size === 0 ? (
              <div className={styles.emptySearch}>No settings match "{search}"</div>
            ) : (
              Array.from(groupedResults.entries()).map(([panelId, rows]) => {
                const panel = PANELS.find((p) => p.id === panelId);
                return (
                  <div key={panelId} className={styles.searchGroup}>
                    <div className={styles.searchGroupTitle}>{panel?.label ?? panelId}</div>
                    {rows.map((row) => <div key={row.id}>{row.render(markDirty)}</div>)}
                  </div>
                );
              })
            )
          ) : activePanel.custom ? (
            activePanel.custom(markDirty)
          ) : (
            // Normal panel view — render all rows for the selected panel
            <div className={styles.panelContent}>
              {panelRows.map((row) => <div key={row.id}>{row.render(markDirty)}</div>)}
            </div>
          )}
        </div>

        {/* ── Footer — always visible ── */}
        <div className={styles.footer}>
          <button className={styles.footerBtn} onClick={handleReset} disabled={!isDirty}>Reset to Defaults</button>
          <button className={styles.footerBtn} onClick={handleSave} disabled={!isDirty}>
            Save Changes
          </button>
        </div>
      </div>
    </div>
  );
}
