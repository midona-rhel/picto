/**
 * Settings window — data-driven settings registry.
 *
 * Every setting is a row with { id, label, keywords, panel, render }.
 * Normal mode: sidebar nav selects a panel, content shows that panel's rows.
 * Search mode (>2 chars): filters ALL rows, groups results by panel name.
 * Footer always visible: Reset (revert to defaults) + Save (commit changes).
 */

import { useState, useMemo, useEffect, useCallback, useRef, type ReactNode } from 'react';
import { IconSettings2, IconCommand, IconPalette, IconX, IconSearch, IconBorderAll, IconLayoutBoard, IconSortAscending, IconSortDescending } from '@tabler/icons-react';
import { getKeyboardPreset, setKeyboardPreset, type KeyboardPreset } from '../../shared/lib/shortcuts';
import { CmSelect } from '../../shared/ui/CmSelect/CmSelect';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';
import { ShortcutsPanel } from './ShortcutsPanel';
import * as api from '../../platform/api';
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
  { id: 'appearance', label: 'Appearance', icon: IconPalette },
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


// ── Reusable row ──

function TreeGuidesToggle({ on, onToggle }: { on: boolean; onToggle: () => void }) {
  return <ToggleSwitch on={on} onChange={onToggle} />;
}

function Row({ label, sep, children }: { label: string; sep?: boolean; children: ReactNode }) {
  return (
    <>
      {sep && <div className={styles.rowSep} />}
      <div className={styles.settingRow}>
        <label className={styles.settingLabel}>{label}</label>
        <div className={styles.settingControl}>{children}</div>
      </div>
    </>
  );
}

// ── Appearance panel ──

const THEMES = [
  { name: 'Auto', css: 'auto', color: undefined },
  { name: 'Light', css: 'light', color: '#ffffff' },
  { name: 'Light Gray', css: 'lightgray', color: '#c8cacd' },
  { name: 'Gray', css: 'gray', color: '#444444' },
  { name: 'Dark', css: 'dark', color: '#010101' },
  { name: 'Blue', css: 'blue', color: '#28356e' },
  { name: 'Purple', css: 'purple', color: '#463275' },
] as const;

const ZOOM_OPTIONS = [
  { value: '75', label: '75%' },
  { value: '80', label: '80%' },
  { value: '90', label: '90%' },
  { value: '100', label: '100%' },
  { value: '110', label: '110%' },
  { value: '125', label: '125%' },
  { value: '150', label: '150%' },
];

function LayoutIcon({ mode }: { mode: string }) {
  if (mode === 'grid') return <IconBorderAll size={14} />;
  if (mode === 'justified') return <IconLayoutBoard size={14} style={{ transform: 'rotate(-90deg)' }} />;
  return <IconLayoutBoard size={14} />;
}

const LAYOUT_OPTIONS = [
  { value: 'waterfall', label: 'Waterfall', icon: <LayoutIcon mode="waterfall" /> },
  { value: 'grid', label: 'Grid', icon: <LayoutIcon mode="grid" /> },
  { value: 'justified', label: 'Justified', icon: <LayoutIcon mode="justified" /> },
];
const SORT_FIELD_OPTIONS = [
  { value: 'date_added', label: 'Date Added' },
  { value: 'date_created', label: 'Date Created' },
  { value: 'date_modified', label: 'Date Modified' },
  { value: 'name', label: 'Name' },
  { value: 'rating', label: 'Rating' },
  { value: 'size_bytes', label: 'File Size' },
  { value: 'duration', label: 'Duration' },
];
const SORT_DIR_OPTIONS = [
  { value: 'asc', label: 'Ascending', icon: <IconSortAscending size={14} /> },
  { value: 'desc', label: 'Descending', icon: <IconSortDescending size={14} /> },
];

function AppearancePanel({ onDirty, appSettings, setAppSettings, prefs, setPrefs }: {
  onDirty: () => void;
  appSettings: api.AppSettings | null;
  setAppSettings: React.Dispatch<React.SetStateAction<api.AppSettings | null>>;
  prefs: api.ViewPrefsDto | null;
  setPrefs: React.Dispatch<React.SetStateAction<api.ViewPrefsDto | null>>;
}) {
  const [activeTheme, setActiveTheme] = useState(() => {
    const saved = localStorage.getItem('picto-theme') ?? 'dark';
    // Apply theme to this window on mount (settings is a separate Electron window)
    const lightThemes = new Set(['light', 'lightgray']);
    const resolved = saved === 'auto'
      ? (window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')
      : saved;
    document.documentElement.dataset.theme = saved === 'auto' ? '' : saved;
    document.documentElement.dataset.mantineColorScheme = lightThemes.has(resolved) ? 'light' : 'dark';
    document.documentElement.style.colorScheme = lightThemes.has(resolved) ? 'light' : 'dark';
    return saved;
  });
  const [zoom, setZoom] = useState('100');

  const updateAppSetting = (patch: Partial<api.AppSettings>) => {
    setAppSettings((cur) => cur ? { ...cur, ...patch } : null);
    // Apply immediately for live preview in the main window
    void api.saveSettings(patch).catch(() => {});
    onDirty();
  };

  const handleThemeChange = (css: string) => {
    // Theme preview is applied immediately (visual feedback), but only persisted on Save
    const lightThemes = new Set(['light', 'lightgray']);
    const resolved = css === 'auto'
      ? (window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')
      : css;
    document.documentElement.dataset.theme = css === 'auto' ? '' : css;
    document.documentElement.dataset.mantineColorScheme = lightThemes.has(resolved) ? 'light' : 'dark';
    document.documentElement.style.colorScheme = lightThemes.has(resolved) ? 'light' : 'dark';
    localStorage.setItem('picto-theme', css);
    setActiveTheme(css);
    updateAppSetting({ colorScheme: css });
  };

  const handleZoomChange = (value: string) => {
    setZoom(value);
    // Zoom preview is applied immediately
    const factor = Number(value) / 100;
    void api.setZoomFactor(factor).catch(() => {});
    updateAppSetting({ zoomFactor: Number(value) / 100 });
  };

  const updateViewPref = (patch: api.ViewPrefsPatch) => {
    setPrefs((cur) => cur ? { ...cur, ...patch } as api.ViewPrefsDto : null);
    // Apply immediately for live preview
    void api.setViewPrefs('', patch).catch(() => {});
    onDirty();
  };

  return (
    <div className={styles.panelContent}>
      {/* ── Appearance ── */}
      <div className={styles.settingsBlock}>
        <div className={styles.blockTitle}>Appearance</div>
        <div className={styles.blockContent}>
          <Row label="Theme">
            <div className={styles.themesPicker}>
              {THEMES.map((t) => (
                <button
                  key={t.css}
                  className={`${styles.themeSwatch} ${t.css === 'auto' ? styles.themeSwatchAuto : ''} ${activeTheme === t.css ? styles.themeSwatchActive : ''}`}
                  style={t.color ? { backgroundColor: t.color } : undefined}
                  title={t.name}
                  type="button"
                  onClick={() => handleThemeChange(t.css)}
                />
              ))}
            </div>
          </Row>
          <div className={styles.rowSep} />
          <div className={styles.labelItems}>
            <div className={styles.labelItem}>
              <label className={styles.settingLabel}>Language</label>
              <div className={styles.settingControl}>
                <CmSelect value="en" options={[{ value: 'en', label: 'English' }]} onChange={() => {}} />
              </div>
            </div>
            <div className={styles.labelItemsSep} />
            <div className={styles.labelItem}>
              <label className={styles.settingLabel}>Zoom</label>
              <div className={styles.settingControl}>
                <CmSelect value={zoom} options={ZOOM_OPTIONS} onChange={handleZoomChange} />
              </div>
            </div>
          </div>
          <Row label="Folder tree guides" sep>
            <TreeGuidesToggle
              on={appSettings?.showTreeGuides ?? true}
              onToggle={() => updateAppSetting({ showTreeGuides: !(appSettings?.showTreeGuides ?? true) })}
            />
          </Row>
        </div>
      </div>

      {/* ── Grid Defaults ── */}
      {prefs && (
        <div className={styles.settingsBlock}>
          <div className={styles.blockTitle}>Grid Defaults</div>
          <div className={styles.blockContent}>
            <Row label="Default layout">
              <CmSelect value={prefs.view_mode ?? 'waterfall'} options={LAYOUT_OPTIONS} onChange={(v) => updateViewPref({ view_mode: v })} />
            </Row>
            <Row label="Thumbnail size" sep>
              <input type="range" min={100} max={600} step={50} value={prefs.target_size ?? 220}
                onChange={(e) => updateViewPref({ target_size: Number(e.target.value) })} className={styles.rangeInput} />
              <span className={styles.valueLabel}>{prefs.target_size ?? 220}px</span>
            </Row>
            <div className={styles.rowSep} />
            <div className={styles.labelItems}>
              <div className={styles.labelItem}>
                <label className={styles.settingLabel}>Sort by</label>
                <div className={styles.settingControl}>
                  <CmSelect value={prefs.sort_field ?? 'date_added'} options={SORT_FIELD_OPTIONS} onChange={(v) => updateViewPref({ sort_field: v })} />
                </div>
              </div>
              <div className={styles.labelItemsSep} />
              <div className={styles.labelItem}>
                <label className={styles.settingLabel}>Order</label>
                <div className={styles.settingControl}>
                  <CmSelect value={prefs.sort_order ?? 'desc'} options={SORT_DIR_OPTIONS} onChange={(v) => updateViewPref({ sort_order: v })} />
                </div>
              </div>
            </div>
            <Row label="Fit thumbnails" sep>
              <ToggleSwitch on={prefs.thumbnail_fit === 'cover'} onChange={() => {
                updateViewPref({ thumbnail_fit: prefs.thumbnail_fit === 'cover' ? 'contain' : 'cover' });
              }} />
            </Row>
            <Row label="Show name" sep>
              <ToggleSwitch on={prefs.show_name ?? true} onChange={() => updateViewPref({ show_name: !(prefs.show_name ?? true) })} />
            </Row>
            <Row label="Show resolution" sep>
              <ToggleSwitch on={prefs.show_resolution ?? false} onChange={() => updateViewPref({ show_resolution: !(prefs.show_resolution ?? false) })} />
            </Row>
            <Row label="Show extension" sep>
              <ToggleSwitch on={prefs.show_extension ?? false} onChange={() => updateViewPref({ show_extension: !(prefs.show_extension ?? false) })} />
            </Row>
            <Row label="Show label" sep>
              <ToggleSwitch on={prefs.show_label ?? false} onChange={() => updateViewPref({ show_label: !(prefs.show_label ?? false) })} />
            </Row>
          </div>
        </div>
      )}
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
];

// ── Component ──

// Default app settings (used by Reset)
const DEFAULT_APP_SETTINGS: Partial<api.AppSettings> = {
  showTreeGuides: true,
  colorScheme: 'dark',
};
const DEFAULT_VIEW_PREFS: api.ViewPrefsPatch = {
  view_mode: 'waterfall',
  target_size: 220,
  sort_field: 'date_added',
  sort_order: 'desc',
  show_name: true,
  show_resolution: false,
  show_extension: false,
  show_label: false,
  thumbnail_fit: 'contain',
};

export function Settings() {
  const [selected, setSelected] = useState('general');
  const [search, setSearch] = useState('');
  const [isDirty, setIsDirty] = useState(false);

  // Working state — applied to backend immediately for live preview.
  // On Save: becomes the new saved baseline.
  // On close without save: reverted to savedSnapshot.
  const [pendingAppSettings, setPendingAppSettings] = useState<api.AppSettings | null>(null);
  const [pendingViewPrefs, setPendingViewPrefs] = useState<api.ViewPrefsDto | null>(null);
  const savedSnapshotRef = useRef<{ app: api.AppSettings | null; prefs: api.ViewPrefsDto | null }>({ app: null, prefs: null });

  // Load from backend on mount + snapshot the saved state
  useEffect(() => {
    void api.getSettings().then((s) => {
      setPendingAppSettings(s);
      savedSnapshotRef.current.app = structuredClone(s);
    }).catch(() => {});
    void api.getViewPrefs('').then((p) => {
      setPendingViewPrefs(p);
      savedSnapshotRef.current.prefs = structuredClone(p);
    }).catch(() => {});
  }, []);

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

  // Revert unsaved changes when window closes (native close button, Cmd+W, etc.)
  useEffect(() => {
    if (!isDirty) return;
    const handler = () => {
      const snap = savedSnapshotRef.current;
      if (snap.app) void api.saveSettings(snap.app).catch(() => {});
      if (snap.prefs) void api.setViewPrefs('', snap.prefs as api.ViewPrefsPatch).catch(() => {});
    };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [isDirty]);

  const handleClose = () => {
    if (isDirty) {
      // Revert to saved snapshot
      const snap = savedSnapshotRef.current;
      if (snap.app) void api.saveSettings(snap.app).catch(() => {});
      if (snap.prefs) void api.setViewPrefs('', snap.prefs as api.ViewPrefsPatch).catch(() => {});
    }
    window.close();
  };

  const handleSave = () => {
    // Persist pending state to backend — this is now the saved baseline
    if (pendingAppSettings) {
      void api.saveSettings(pendingAppSettings).catch(() => {});
      savedSnapshotRef.current.app = structuredClone(pendingAppSettings);
    }
    if (pendingViewPrefs) {
      void api.setViewPrefs('', pendingViewPrefs as api.ViewPrefsPatch).catch(() => {});
      savedSnapshotRef.current.prefs = structuredClone(pendingViewPrefs);
    }
    setIsDirty(false);
  };

  const handleReset = () => {
    if (!window.confirm('Reset all settings to defaults?')) return;
    setKeyboardPreset('us');
    setPendingAppSettings((cur) => cur ? { ...cur, ...DEFAULT_APP_SETTINGS } : null);
    setPendingViewPrefs((cur) => cur ? { ...cur, ...DEFAULT_VIEW_PREFS } as api.ViewPrefsDto : null);
    setIsDirty(true);
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
          ) : activePanel.id === 'appearance' ? (
            <AppearancePanel
              onDirty={markDirty}
              appSettings={pendingAppSettings}
              setAppSettings={setPendingAppSettings}
              prefs={pendingViewPrefs}
              setPrefs={setPendingViewPrefs}
            />
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
