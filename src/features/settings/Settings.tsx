/**
 * Settings window — data-driven settings registry.
 *
 * Every setting is a row with { id, label, keywords, panel, render }.
 * Normal mode: sidebar nav selects a panel, content shows that panel's rows.
 * Search mode (>2 chars): filters ALL rows, groups results by panel name.
 * Footer always visible: Save closes after committing; Apply commits in place.
 */

import { useState, useMemo, useEffect, useCallback, useRef, type ReactNode } from 'react';
import {
  IconAdjustmentsHorizontal,
  IconBell,
  IconBorderAll,
  IconBox,
  IconCheck,
  IconCommand,
  IconEye,
  IconFolderDown,
  IconDownload,
  IconLayoutBoard,
  IconLayoutSidebar,
  IconSearch,
  IconSettings2,
  IconSortAscending,
  IconSortDescending,
  IconX,
} from '@tabler/icons-react';
import {
  getKeyboardPreset,
  getShortcutOverrides,
  persistShortcutState,
  replaceShortcutOverrides,
  setKeyboardPreset,
  type KeyboardPreset,
  type ShortcutBindingOverride,
} from '../../shared/lib/shortcuts';
import { CmSelect } from '../../shared/ui/CmSelect/CmSelect';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';
import { ShortcutsPanel } from './ShortcutsPanel';
import { AiTaggingPanel } from './AiTaggingPanel';
import { appController } from '../../controllers/appController';
import { settingsController, type AppSettings, type ViewPrefsDto, type ViewPrefsPatch } from '../../controllers/settingsController';
import { previewTheme, themeNeedsNativeWindowRestart } from '../../runtime/themeRuntime';
import {
  AUDIO_VISUALIZATION_OPTIONS,
  getAudioVisualizationMode,
  setAudioVisualizationMode,
  type AudioVisualizationMode,
} from '../../shared/lib/audioVisualization';
import styles from './Settings.module.css';
import type { NotificationTone } from '../../shared/lib/notifications';

// ── Settings row definition ──

interface SettingRow {
  id: string;
  label: string;
  keywords: string;
  panel: string;
}

// ── Panel definitions ──

interface PanelDef {
  id: string;
  label: string;
  icon: typeof IconSettings2;
  /** Terms covered by this category, used by the single settings search path. */
  keywords: string;
  description: string;
  separatorBefore?: boolean;
}

const PANELS: PanelDef[] = [
  {
    id: 'general', label: 'General', icon: IconSettings2,
    keywords: 'general appearance theme color light dark gray blue purple zoom',
    description: 'Appearance and zoom.',
  },
  {
    id: 'sidebar', label: 'Sidebar', icon: IconLayoutSidebar,
    keywords: 'sidebar navigation folder tree guides hierarchy lines',
    description: 'Sidebar and folder-tree presentation.',
  },
  {
    id: 'controls', label: 'Controls', icon: IconAdjustmentsHorizontal,
    keywords: 'controls grid layout thumbnails spacing density wide tight sort order name resolution extension label count fit',
    description: 'Default grid controls and item presentation.',
    separatorBefore: true,
  },
  {
    id: 'preview', label: 'Preview', icon: IconEye,
    keywords: 'preview image video audio scaling pixelated zoom transparency autoplay loop visualization visualizer spectrum oscilloscope orbit',
    description: 'Media preview behavior.',
  },
  {
    id: 'shortcuts', label: 'Shortcuts', icon: IconCommand,
    keywords: 'shortcuts keyboard shortcut keybind hotkey binding command key layout qwerty qwertz azerty eu us european preset',
    description: 'Keyboard layout and shortcut bindings.',
  },
  {
    id: 'notifications', label: 'Notifications', icon: IconBell,
    keywords: 'notifications alerts popups success information warnings errors',
    description: 'Choose which actions show pop-up notifications.',
    separatorBefore: true,
  },
  {
    id: 'autoimport', label: 'Auto-Import', icon: IconFolderDown,
    keywords: 'auto import watched folder watch files inbox recursive subfolders',
    description: 'Control imports from watched folders.',
  },
  {
    id: 'subscriptions', label: 'Subscriptions', icon: IconDownload,
    keywords: 'subscriptions defaults schedule daily weekly monthly posts run group multi media',
    description: 'Defaults for new subscriptions and source queries.',
  },
  {
    id: 'aitagging', label: 'AI Models', icon: IconBox,
    keywords: 'ai tagging tagger models model download threshold confidence auto tag rating',
    description: 'Local models, confidence thresholds, and auto-tag behavior.',
    separatorBefore: true,
  },
];

// ── Individual setting rows (for General + future panels) ──

function KeyboardPresetRow({ preset, onChange }: { preset: KeyboardPreset; onChange: (preset: KeyboardPreset) => void }) {
  return (
    <div className={styles.settingsBlock}>
      <div className={styles.blockContent}>
        <div className={styles.blockTitle}>Keyboard</div>
        <Row label="Keyboard layout">
          <CmSelect
            value={preset}
            options={[
              { value: 'us', label: 'US (QWERTY)' },
              { value: 'eu', label: 'EU (QWERTZ / AZERTY / Nordic)' },
            ]}
            onChange={(value) => onChange(value as KeyboardPreset)}
            width={260}
            ariaLabel="Keyboard layout"
          />
        </Row>
        <p className={styles.settingHint}>
          EU mode adds alternatives for shortcuts that use backtick, backslash, and brackets.
        </p>
      </div>
    </div>
  );
}


// ── Reusable rows ──

function CheckSetting({ checked, disabled = false, label, onChange }: {
  checked: boolean;
  disabled?: boolean;
  label: string;
  onChange: () => void;
}) {
  return (
    <label className={`${styles.checkboxItem} ${disabled ? styles.checkboxItemDisabled : ''}`}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={onChange} />
      <span className={styles.checkboxIndicator} aria-hidden="true">
        {checked ? <IconCheck size={11} stroke={3} /> : null}
      </span>
      <span>{label}</span>
    </label>
  );
}

function RadioSetting({ checked, label, name, onChange }: {
  checked: boolean;
  label: string;
  name: string;
  onChange: () => void;
}) {
  return (
    <label className={styles.checkboxItem}>
      <input type="radio" name={name} checked={checked} onChange={onChange} />
      <span className={`${styles.checkboxIndicator} ${styles.radioIndicator}`} aria-hidden="true" />
      <span>{label}</span>
    </label>
  );
}

function Row({ label, sep, children }: { label: string; sep?: boolean; children: ReactNode }) {
  return (
    <>
      {sep && <div className={styles.rowSep} />}
      <div className={styles.settingRow}>
        <span className={styles.settingLabel}>{label}</span>
        <div className={styles.settingControl}>{children}</div>
      </div>
    </>
  );
}

function IntegerSettingInput({ value, min, max, label, onChange }: {
  value: number;
  min: number;
  max: number;
  label: string;
  onChange: (value: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));

  useEffect(() => setDraft(String(value)), [value]);

  const commit = (next: string) => {
    const parsed = Number(next);
    if (Number.isSafeInteger(parsed) && parsed >= min && parsed <= max) onChange(parsed);
  };

  return (
    <input
      aria-label={label}
      className={styles.numberInput}
      min={min}
      max={max}
      step={1}
      type="number"
      value={draft}
      onBlur={() => setDraft(String(value))}
      onChange={(event) => {
        setDraft(event.target.value);
        commit(event.target.value);
      }}
    />
  );
}

// ── Appearance panel ──

const THEMES: Array<{ name: string; css: string; color: string | undefined }> = [
  { name: 'Auto', css: 'auto', color: undefined },
  { name: 'Light', css: 'light', color: '#ffffff' },
  { name: 'Light Gray', css: 'lightgray', color: '#c8cacd' },
  { name: 'Gray', css: 'gray', color: '#444444' },
  { name: 'Dark', css: 'dark', color: '#010101' },
  { name: 'Blue', css: 'blue', color: '#28356e' },
  { name: 'Purple', css: 'purple', color: '#463275' },
];

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
  { value: 'imported_at', label: 'Date Added' },
  { value: 'captured_at', label: 'Date Created' },
  { value: 'name', label: 'Name' },
  { value: 'rating', label: 'Rating' },
  { value: 'size', label: 'File Size' },
  { value: 'random', label: 'Random' },
];
const SORT_DIR_OPTIONS = [
  { value: 'ascending', label: 'Ascending', icon: <IconSortAscending size={14} /> },
  { value: 'descending', label: 'Descending', icon: <IconSortDescending size={14} /> },
];
const GRID_SPACING_OPTIONS = [
  { value: 'wide', label: 'Wide' },
  { value: 'tight', label: 'Tight' },
];
const WHEEL_ACTION_OPTIONS = [
  { value: 'scroll', label: 'Scroll grid' },
  { value: 'zoom', label: 'Adjust thumbnail size' },
];
const MEDIA_GESTURE_OPTIONS = [
  { value: 'wheel_zoom', label: 'Wheel zoom' },
  { value: 'trackpad', label: 'Trackpad pan + pinch zoom' },
];
const CONTROL_SELECT_WIDTH = 220;
const DOUBLE_CLICK_ACTION_OPTIONS = [
  { value: 'detail', label: 'Open Media View' },
  { value: 'external', label: 'Open in default app' },
];
const MIDDLE_CLICK_ACTION_OPTIONS = [
  { value: 'new_window', label: 'Open in new window' },
  { value: 'none', label: 'Do nothing' },
];
const SPACE_ACTION_OPTIONS = [
  { value: 'quick_look', label: 'Quick Look' },
  { value: 'scroll', label: 'Scroll page' },
];
const IMAGE_RENDERING_OPTIONS = [
  { value: 'smooth', label: 'Smooth' },
  { value: 'pixelated', label: 'Pixelated' },
];
const IMAGE_DEFAULT_ZOOM_OPTIONS = [
  { value: 'fit', label: 'Fit to window' },
  { value: 'actual', label: 'Actual size' },
];

const NOTIFICATION_TONE_OPTIONS: Array<{ tone: NotificationTone; label: string }> = [
  { tone: 'success', label: 'Successful actions' },
  { tone: 'info', label: 'Information' },
  { tone: 'warning', label: 'Warnings' },
  { tone: 'error', label: 'Errors' },
];

const SUBSCRIPTION_SCHEDULE_OPTIONS = [
  { value: 'manual', label: 'Manual' },
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
];

function PreferencePanel({ panel, onDirty, appSettings, setAppSettings, prefs, setPrefs, audioVisualization, setAudioVisualization }: {
  panel: 'general' | 'sidebar' | 'controls' | 'preview' | 'notifications' | 'autoimport' | 'subscriptions';
  onDirty: () => void;
  appSettings: AppSettings | null;
  setAppSettings: React.Dispatch<React.SetStateAction<AppSettings | null>>;
  prefs: ViewPrefsDto | null;
  setPrefs: React.Dispatch<React.SetStateAction<ViewPrefsDto | null>>;
  audioVisualization: AudioVisualizationMode;
  setAudioVisualization: (mode: AudioVisualizationMode) => void;
}) {
  const [zoom, setZoom] = useState('100');
  const activeTheme = appSettings?.colorScheme ?? 'dark';
  const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

  useEffect(() => {
    if (appSettings?.zoomFactor == null) {
      setZoom('100');
      return;
    }
    const percentage = Math.round(appSettings.zoomFactor * 100);
    if (ZOOM_OPTIONS.some((option) => option.value === String(percentage))) {
      setZoom(String(percentage));
    }
  }, [appSettings?.zoomFactor]);

  const updateAppSetting = (patch: Partial<AppSettings>) => {
    setAppSettings((cur) => cur ? { ...cur, ...patch } : null);
    onDirty();
  };

  const handleThemeChange = (css: string) => {
    setAppSettings((current) => current ? { ...current, colorScheme: css } : null);
    previewTheme(css);
    onDirty();
  };

  const handleZoomChange = (value: string) => {
    setZoom(value);
    updateAppSetting({ zoomFactor: Number(value) / 100 });
  };

  const handleAudioVisualizationChange = (value: string) => {
    const mode = value as AudioVisualizationMode;
    setAudioVisualization(mode);
    setAudioVisualizationMode(mode, false);
    onDirty();
  };

  const toggleNotificationTone = (tone: NotificationTone) => {
    const tones = appSettings?.notificationPopupTones ?? [];
    updateAppSetting({
      notificationPopupTones: tones.includes(tone)
        ? tones.filter((candidate) => candidate !== tone)
        : [...tones, tone],
    });
  };

  const updateViewPref = (patch: ViewPrefsPatch) => {
    setPrefs((cur) => cur ? { ...cur, ...patch } as ViewPrefsDto : null);
    onDirty();
  };

  return (
    <div className={styles.panelContent}>
      {panel === 'general' ? (
        <>
          <div className={styles.settingsBlock}>
            <div className={styles.blockContent}>
              <div className={styles.blockTitle}>Appearance</div>
              <Row label="Theme">
                <div className={styles.themesPicker}>
                  {THEMES.map((t) => (
                    <button
                      key={t.css}
                      className={`${styles.themeSwatch} ${t.css === 'auto' ? styles.themeSwatchAuto : ''} ${!t.color && t.css !== 'auto' ? styles.themeSwatchGlass : ''} ${activeTheme === t.css ? styles.themeSwatchActive : ''}`}
                      style={t.color ? { backgroundColor: t.color } : undefined}
                      data-tooltip={t.name}
                      type="button"
                      aria-label={`${t.name} theme`}
                      onClick={() => handleThemeChange(t.css)}
                    />
                  ))}
                </div>
              </Row>
              <div className={styles.rowSep} />
              <Row label="Zoom Level">
                <CmSelect value={zoom} options={ZOOM_OPTIONS} onChange={handleZoomChange} />
              </Row>
            </div>
          </div>
          <div className={styles.settingsBlock}>
            <div className={styles.blockContent}>
              <div className={styles.blockTitle}>Interface</div>
              <CheckSetting
                checked={appSettings?.showSidebarCounts ?? true}
                label="Show item counts in the sidebar"
                onChange={() => updateAppSetting({ showSidebarCounts: !(appSettings?.showSidebarCounts ?? true) })}
              />
            </div>
          </div>
        </>
      ) : null}

      {panel === 'sidebar' ? (
        <>
          <div className={styles.settingsBlock}>
            <div className={styles.blockContent}>
              <div className={styles.blockTitle}>Double Click</div>
              <div className={styles.sidebarSettingsGrid}>
                <RadioSetting
                  checked={(appSettings?.sidebarDoubleClickAction ?? 'collapse') === 'rename'}
                  label="Rename"
                  name="sidebar-double-click"
                  onChange={() => updateAppSetting({ sidebarDoubleClickAction: 'rename' })}
                />
                <RadioSetting
                  checked={(appSettings?.sidebarDoubleClickAction ?? 'collapse') === 'collapse'}
                  label="Expand/Collapse"
                  name="sidebar-double-click"
                  onChange={() => updateAppSetting({ sidebarDoubleClickAction: 'collapse' })}
                />
              </div>
            </div>
          </div>
          <div className={styles.settingsBlock}>
            <div className={styles.blockContent}>
              <div className={styles.blockTitle}>Show these items in the sidebar:</div>
              <div className={styles.sidebarSettingsGrid}>
                <CheckSetting checked disabled label="All" onChange={() => {}} />
                <CheckSetting checked={appSettings?.showSidebarUncategorized ?? true} label="Uncategorized" onChange={() => updateAppSetting({ showSidebarUncategorized: !(appSettings?.showSidebarUncategorized ?? true) })} />
                <CheckSetting checked={appSettings?.showSidebarUntagged ?? true} label="Untagged" onChange={() => updateAppSetting({ showSidebarUntagged: !(appSettings?.showSidebarUntagged ?? true) })} />
                <CheckSetting checked={appSettings?.showSidebarInbox ?? true} label="Inbox" onChange={() => updateAppSetting({ showSidebarInbox: !(appSettings?.showSidebarInbox ?? true) })} />
                <CheckSetting checked={appSettings?.showSidebarRandom ?? true} label="Random" onChange={() => updateAppSetting({ showSidebarRandom: !(appSettings?.showSidebarRandom ?? true) })} />
                <CheckSetting checked={appSettings?.showSidebarTagManager ?? true} label="Tag Manager" onChange={() => updateAppSetting({ showSidebarTagManager: !(appSettings?.showSidebarTagManager ?? true) })} />
                <CheckSetting checked={appSettings?.showSidebarRecentlyViewed ?? true} label="Recently Viewed" onChange={() => updateAppSetting({ showSidebarRecentlyViewed: !(appSettings?.showSidebarRecentlyViewed ?? true) })} />
                <CheckSetting checked={appSettings?.showSidebarSubscriptions ?? true} label="Subscriptions" onChange={() => updateAppSetting({ showSidebarSubscriptions: !(appSettings?.showSidebarSubscriptions ?? true) })} />
                <CheckSetting checked={appSettings?.showSidebarDuplicates ?? true} label="Duplicates" onChange={() => updateAppSetting({ showSidebarDuplicates: !(appSettings?.showSidebarDuplicates ?? true) })} />
                <CheckSetting checked disabled label="Trash" onChange={() => {}} />
              </div>
              <div className={styles.rowSep} />
              <div className={styles.sidebarSettingsGrid}>
                <CheckSetting checked={appSettings?.showSidebarQuickAccess ?? true} label="Quick Access" onChange={() => updateAppSetting({ showSidebarQuickAccess: !(appSettings?.showSidebarQuickAccess ?? true) })} />
                <CheckSetting checked={appSettings?.showSidebarSmartFolders ?? true} label="Smart Folders" onChange={() => updateAppSetting({ showSidebarSmartFolders: !(appSettings?.showSidebarSmartFolders ?? true) })} />
                <CheckSetting checked={appSettings?.showSidebarFolders ?? true} label="Folders" onChange={() => updateAppSetting({ showSidebarFolders: !(appSettings?.showSidebarFolders ?? true) })} />
              </div>
            </div>
          </div>
          <div className={styles.settingsBlock}>
            <div className={styles.blockContent}>
              <div className={styles.blockTitle}>Folder Tree</div>
              <CheckSetting
                checked={appSettings?.showTreeGuides ?? true}
                label="Show hierarchy guides"
                onChange={() => updateAppSetting({ showTreeGuides: !(appSettings?.showTreeGuides ?? true) })}
              />
            </div>
          </div>
        </>
      ) : null}

      {panel === 'preview' ? (
        <>
          <div className={styles.settingsBlock}>
            <div className={styles.blockContent}>
              <div className={styles.blockTitle}>Image</div>
              <Row label="Scaling">
                <CmSelect
                  value={appSettings?.imageRendering ?? 'smooth'}
                  options={IMAGE_RENDERING_OPTIONS}
                  onChange={(value) => updateAppSetting({ imageRendering: value as AppSettings['imageRendering'] })}
                />
              </Row>
              <Row label="Default zoom" sep>
                <CmSelect
                  value={appSettings?.imageDefaultZoom ?? 'fit'}
                  options={IMAGE_DEFAULT_ZOOM_OPTIONS}
                  onChange={(value) => updateAppSetting({ imageDefaultZoom: value as AppSettings['imageDefaultZoom'] })}
                />
              </Row>
              <div className={styles.rowSep} />
              <CheckSetting
                checked={appSettings?.showTransparencyGrid ?? false}
                label="Show transparency grid"
                onChange={() => updateAppSetting({ showTransparencyGrid: !(appSettings?.showTransparencyGrid ?? false) })}
              />
            </div>
          </div>
          <div className={styles.settingsBlock}>
            <div className={styles.blockContent}>
              <div className={styles.blockTitle}>Video</div>
              <div className={styles.checkboxGrid}>
                <CheckSetting
                  checked={appSettings?.videoAutoPlay ?? true}
                  label="Autoplay videos"
                  onChange={() => updateAppSetting({ videoAutoPlay: !(appSettings?.videoAutoPlay ?? true) })}
                />
                <CheckSetting
                  checked={appSettings?.videoLoop ?? true}
                  label="Loop videos"
                  onChange={() => updateAppSetting({ videoLoop: !(appSettings?.videoLoop ?? true) })}
                />
              </div>
            </div>
          </div>
          <div className={styles.settingsBlock}>
            <div className={styles.blockContent}>
              <div className={styles.blockTitle}>Audio</div>
              <Row label="Visualization">
                <CmSelect
                  value={audioVisualization}
                  options={AUDIO_VISUALIZATION_OPTIONS}
                  onChange={handleAudioVisualizationChange}
                />
              </Row>
            </div>
          </div>
        </>
      ) : null}

      {panel === 'controls' ? (
        <>
          <div className={styles.settingsBlock}>
            <div className={styles.blockContent}>
              <div className={styles.blockTitle}>Mouse</div>
              <Row label="Mouse wheel"><CmSelect width={CONTROL_SELECT_WIDTH} value={appSettings?.gridWheelAction ?? 'scroll'} options={WHEEL_ACTION_OPTIONS} onChange={(value) => updateAppSetting({ gridWheelAction: value as AppSettings['gridWheelAction'] })} /></Row>
              {isMac ? <Row label="Media view gestures" sep><CmSelect width={CONTROL_SELECT_WIDTH} value={appSettings?.viewerTrackpadGestures ? 'trackpad' : 'wheel_zoom'} options={MEDIA_GESTURE_OPTIONS} onChange={(value) => updateAppSetting({ viewerTrackpadGestures: value === 'trackpad' })} /></Row> : null}
              <Row label="Double-click" sep><CmSelect width={CONTROL_SELECT_WIDTH} value={appSettings?.gridDoubleClickAction ?? 'detail'} options={DOUBLE_CLICK_ACTION_OPTIONS} onChange={(value) => updateAppSetting({ gridDoubleClickAction: value as AppSettings['gridDoubleClickAction'] })} /></Row>
              <Row label="Middle-click" sep><CmSelect width={CONTROL_SELECT_WIDTH} value={appSettings?.gridMiddleClickAction ?? 'new_window'} options={MIDDLE_CLICK_ACTION_OPTIONS} onChange={(value) => updateAppSetting({ gridMiddleClickAction: value as AppSettings['gridMiddleClickAction'] })} /></Row>
            </div>
          </div>
          <div className={styles.settingsBlock}>
            <div className={styles.blockContent}>
              <div className={styles.blockTitle}>Keyboard</div>
              <Row label="Space key"><CmSelect width={CONTROL_SELECT_WIDTH} value={appSettings?.spaceKeyAction ?? 'quick_look'} options={SPACE_ACTION_OPTIONS} onChange={(value) => updateAppSetting({ spaceKeyAction: value as AppSettings['spaceKeyAction'] })} /></Row>
            </div>
          </div>
          {prefs ? (
            <div className={styles.settingsBlock}>
              <div className={styles.blockContent}>
                <div className={styles.blockTitle}>Grid Defaults</div>
                <Row label="Default layout">
                  <CmSelect value={prefs.view_mode ?? 'waterfall'} options={LAYOUT_OPTIONS} onChange={(v) => updateViewPref({ view_mode: v })} />
                </Row>
                <Row label="Thumbnail size" sep>
                  <input type="range" min={100} max={600} step={50} value={prefs.target_size ?? 220}
                    onChange={(e) => updateViewPref({ target_size: Number(e.target.value) })} className={styles.rangeInput} />
                  <span className={styles.valueLabel}>{prefs.target_size ?? 220}px</span>
                </Row>
                <Row label="Grid spacing" sep>
                  <CmSelect
                    value={appSettings?.gridSpacing ?? 'wide'}
                    options={GRID_SPACING_OPTIONS}
                    onChange={(value) => updateAppSetting({ gridSpacing: value as 'wide' | 'tight' })}
                  />
                </Row>
                <div className={styles.rowSep} />
                <div className={styles.labelItems}>
                  <div className={styles.labelItem}>
                    <label className={styles.settingLabel}>Sort by</label>
                    <div className={styles.settingControl}>
                      <CmSelect value={prefs.sort_field ?? 'imported_at'} options={SORT_FIELD_OPTIONS} onChange={(v) => updateViewPref({ sort_field: v })} />
                    </div>
                  </div>
                  <div className={styles.labelItemsSep} />
                  <div className={styles.labelItem}>
                    <label className={styles.settingLabel}>Order</label>
                    <div className={styles.settingControl}>
                      <CmSelect value={prefs.sort_order ?? 'descending'} options={SORT_DIR_OPTIONS} onChange={(v) => updateViewPref({ sort_order: v })} />
                    </div>
                  </div>
                </div>
                <div className={styles.rowSep} />
                <div className={styles.checkboxGrid}>
                  <CheckSetting checked={prefs.thumbnail_fit === 'cover'} label="Fit thumbnails" onChange={() => {
                    updateViewPref({ thumbnail_fit: prefs.thumbnail_fit === 'cover' ? 'contain' : 'cover' });
                  }} />
                  <CheckSetting checked={prefs.show_name ?? true} label="Show name" onChange={() => updateViewPref({ show_name: !(prefs.show_name ?? true) })} />
                  <CheckSetting checked={prefs.show_resolution ?? false} label="Show resolution" onChange={() => updateViewPref({ show_resolution: !(prefs.show_resolution ?? false) })} />
                  <CheckSetting checked={prefs.show_extension ?? false} label="Show extension" onChange={() => updateViewPref({ show_extension: !(prefs.show_extension ?? false) })} />
                  <CheckSetting checked={prefs.show_label ?? false} label="Show label" onChange={() => updateViewPref({ show_label: !(prefs.show_label ?? false) })} />
                  <CheckSetting checked={prefs.show_item_count ?? true} label="Show item count" onChange={() => updateViewPref({ show_item_count: !(prefs.show_item_count ?? true) })} />
                </div>
              </div>
            </div>
          ) : null}
        </>
      ) : null}

      {panel === 'notifications' ? (
        <div className={styles.settingsBlock}>
          <div className={styles.blockContent}>
            <div className={styles.blockTitle}>
              <span>Pop-ups</span>
              <span className={styles.blockTitleControl}>
                <ToggleSwitch
                  on={appSettings?.notificationPopupsEnabled ?? true}
                  onChange={() => updateAppSetting({ notificationPopupsEnabled: !(appSettings?.notificationPopupsEnabled ?? true) })}
                />
              </span>
            </div>
            {(appSettings?.notificationPopupsEnabled ?? true) ? (
              <div className={styles.checkboxGrid}>
                {NOTIFICATION_TONE_OPTIONS.map(({ tone, label }) => (
                  <CheckSetting
                    key={tone}
                    checked={(appSettings?.notificationPopupTones ?? []).includes(tone)}
                    label={label}
                    onChange={() => toggleNotificationTone(tone)}
                  />
                ))}
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      {panel === 'autoimport' ? (
        <div className={styles.settingsBlock}>
          <div className={styles.blockContent}>
            <div className={styles.blockTitle}>
              <span>Auto-Import</span>
              <span className={styles.blockTitleControl}>
                <ToggleSwitch
                  on={appSettings?.autoImportEnabled ?? true}
                  onChange={() => updateAppSetting({ autoImportEnabled: !(appSettings?.autoImportEnabled ?? true) })}
                />
              </span>
            </div>
            <p className={styles.settingHint}>
              Import new files into Inbox from watched folders configured in the folder context menu.
            </p>
          </div>
        </div>
      ) : null}

      {panel === 'subscriptions' ? (
        <div className={styles.settingsBlock}>
          <div className={styles.blockContent}>
            <div className={styles.blockTitle}>Defaults for New Subscriptions</div>
            <Row label="Schedule">
              <CmSelect
                value={appSettings?.subscriptionDefaultSchedule ?? 'daily'}
                options={SUBSCRIPTION_SCHEDULE_OPTIONS}
                onChange={(value) => updateAppSetting({
                  subscriptionDefaultSchedule: value as AppSettings['subscriptionDefaultSchedule'],
                })}
              />
            </Row>
            <Row label="Posts per run" sep>
              <IntegerSettingInput
                label="Posts per run"
                min={1}
                max={10_000}
                value={appSettings?.subscriptionDefaultPostsPerRun ?? 100}
                onChange={(value) => updateAppSetting({ subscriptionDefaultPostsPerRun: value })}
              />
            </Row>
            <div className={styles.rowSep} />
            <CheckSetting
              checked={appSettings?.subscriptionDefaultGroupPosts ?? true}
              label="Group multi-media posts"
              onChange={() => updateAppSetting({
                subscriptionDefaultGroupPosts: !(appSettings?.subscriptionDefaultGroupPosts ?? true),
              })}
            />
          </div>
        </div>
      ) : null}
    </div>
  );
}

const ALL_SETTINGS: SettingRow[] = [
  {
    id: 'general.appearance',
    label: 'Appearance',
    keywords: 'theme color light dark gray blue purple zoom',
    panel: 'general',
  },
  {
    id: 'shortcuts.keyboard',
    label: 'Keyboard Layout',
    keywords: 'keyboard layout qwerty qwertz azerty eu us european preset shortcut',
    panel: 'shortcuts',
  },
  {
    id: 'sidebar.folder-tree',
    label: 'Folder Tree',
    keywords: 'sidebar folder hierarchy tree guides lines',
    panel: 'sidebar',
  },
  {
    id: 'controls.grid',
    label: 'Grid Defaults',
    keywords: 'grid layout thumbnail size spacing density wide tight sort name resolution extension label count fit',
    panel: 'controls',
  },
  {
    id: 'preview.image',
    label: 'Image Preview',
    keywords: 'image preview scaling smooth pixelated zoom fit actual transparency checkerboard',
    panel: 'preview',
  },
  {
    id: 'preview.video',
    label: 'Video Preview',
    keywords: 'video preview autoplay loop playback',
    panel: 'preview',
  },
  {
    id: 'preview.audio',
    label: 'Audio Preview',
    keywords: 'audio preview visualization spectrum oscilloscope orbit',
    panel: 'preview',
  },
  {
    id: 'notifications.behavior',
    label: 'Notifications',
    keywords: 'notifications alerts popups success information warnings errors',
    panel: 'notifications',
  },
  {
    id: 'autoimport.behavior',
    label: 'Auto-Import',
    keywords: 'auto import watched folder watch files inbox recursive subfolders',
    panel: 'autoimport',
  },
  {
    id: 'subscriptions.defaults',
    label: 'Subscription Defaults',
    keywords: 'subscriptions schedule daily weekly monthly posts per run group multi media',
    panel: 'subscriptions',
  },
];

// ── Component ──

const GRID_SETTINGS_SCOPE = 'system:active';

export function Settings() {
  const [selected, setSelected] = useState('general');
  const [search, setSearch] = useState('');
  const [isDirty, setIsDirty] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [pendingKeyboardPreset, setPendingKeyboardPreset] = useState<KeyboardPreset>(getKeyboardPreset);
  const [pendingAudioVisualization, setPendingAudioVisualization] = useState<AudioVisualizationMode>(getAudioVisualizationMode);

  const [pendingAppSettings, setPendingAppSettings] = useState<AppSettings | null>(null);
  const [pendingViewPrefs, setPendingViewPrefs] = useState<ViewPrefsDto | null>(null);
  const savedSnapshotRef = useRef<{
    app: AppSettings | null;
    prefs: ViewPrefsDto | null;
    keyboardPreset: KeyboardPreset;
    audioVisualization: AudioVisualizationMode;
    shortcutOverrides: Readonly<Record<string, ShortcutBindingOverride>>;
  }>({
    app: null,
    prefs: null,
    keyboardPreset: getKeyboardPreset(),
    audioVisualization: getAudioVisualizationMode(),
    shortcutOverrides: getShortcutOverrides(),
  });

  // Load from backend on mount + snapshot the saved state
  useEffect(() => {
    void settingsController.getSettings().then((s) => {
      setPendingAppSettings(s);
      savedSnapshotRef.current.app = structuredClone(s);
    }).catch(() => {});
    void settingsController.getViewPrefs(GRID_SETTINGS_SCOPE).then((p) => {
      setPendingViewPrefs(p);
      savedSnapshotRef.current.prefs = structuredClone(p);
    }).catch(() => {});
  }, []);

  const markDirty = useCallback(() => setIsDirty(true), []);

  const activePanel = PANELS.find((p) => p.id === selected) ?? PANELS[0];
  const searchQuery = search.trim().toLowerCase();
  const isSearching = searchQuery.length > 0;

  // Filter settings by search query
  const searchResults = useMemo(() => {
    if (!isSearching) return [];
    const q = searchQuery;
    return ALL_SETTINGS.filter((s) =>
      s.label.toLowerCase().includes(q) || s.keywords.toLowerCase().includes(q),
    );
  }, [searchQuery, isSearching]);

  // Categories own the search vocabulary, so custom panels and registry rows share one path.
  const matchedPanels = useMemo(() => {
    if (!isSearching) return [];
    return PANELS.filter((panel) =>
      panel.label.toLowerCase().includes(searchQuery) ||
      panel.keywords.includes(searchQuery) ||
      searchResults.some((setting) => setting.panel === panel.id),
    );
  }, [isSearching, searchQuery, searchResults]);

  // Filter sidebar nav
  const filteredPanels = useMemo(() => {
    if (!isSearching) return PANELS;
    return matchedPanels;
  }, [isSearching, matchedPanels]);

  const restoreRuntimePreview = useCallback(() => {
    const snapshot = savedSnapshotRef.current;
    setKeyboardPreset(snapshot.keyboardPreset, false);
    setAudioVisualizationMode(snapshot.audioVisualization, false);
    replaceShortcutOverrides(snapshot.shortcutOverrides, false);
    if (snapshot.app) previewTheme(snapshot.app.colorScheme);
  }, []);

  useEffect(() => {
    if (!isDirty) return;
    const handler = () => restoreRuntimePreview();
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [isDirty, restoreRuntimePreview]);

  const handleClose = () => {
    if (isDirty) {
      restoreRuntimePreview();
    }
    window.close();
  };

  const handleSave = async (closeAfterSave: boolean) => {
    if (isSaving) return;
    const needsRestart = pendingAppSettings
      ? themeNeedsNativeWindowRestart(savedSnapshotRef.current.app?.colorScheme, pendingAppSettings.colorScheme)
      : false;
    setIsSaving(true);
    setSaveError(null);
    try {
      if (pendingAppSettings) await settingsController.replaceSettings(pendingAppSettings);
      if (pendingViewPrefs) {
        await settingsController.setViewPrefs(GRID_SETTINGS_SCOPE, settingsController.viewPrefsToPatch(pendingViewPrefs));
      }
      setKeyboardPreset(pendingKeyboardPreset);
      setAudioVisualizationMode(pendingAudioVisualization);
      persistShortcutState();
      savedSnapshotRef.current = {
        app: pendingAppSettings ? structuredClone(pendingAppSettings) : null,
        prefs: pendingViewPrefs ? structuredClone(pendingViewPrefs) : null,
        keyboardPreset: pendingKeyboardPreset,
        audioVisualization: pendingAudioVisualization,
        shortcutOverrides: getShortcutOverrides(),
      };
      setIsDirty(false);
      if (needsRestart) await appController.restartMainWindow();
      if (closeAfterSave) window.close();
    } catch (reason) {
      setSaveError(reason instanceof Error ? reason.message : 'Unable to save settings.');
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className={styles.root}>
      {/* ── Sidebar ── */}
      <div className={styles.sidebar}>
        <div className={styles.sidebarTitle}>Preferences</div>
        <div className={styles.searchWrap}>
          <IconSearch size={14} className={styles.searchIcon} />
          <input
            className={styles.sidebarSearch}
            type="search"
            placeholder="Search..."
            aria-label="Search settings"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <div className={styles.sidebarItems}>
          {filteredPanels.map((panel) => {
            const Icon = panel.icon;
            const isActive = panel.id === selected && !isSearching;
            return (
              <div key={panel.id} className={styles.navEntry}>
                {panel.separatorBefore ? <div className={styles.navSeparator} /> : null}
                <button
                  className={isActive ? styles.navItemActive : styles.navItem}
                  onClick={() => { setSelected(panel.id); setSearch(''); }}
                >
                  <Icon size={18} stroke={1.8} />
                  {panel.label}
                </button>
              </div>
            );
          })}
        </div>
      </div>

      {/* ── Content ── */}
      <div className={styles.content}>
        <div className={styles.contentHeader}>
          <span className={styles.contentTitle}>
            {isSearching ? `Search results for "${search}"` : activePanel.label}
          </span>
          <button className={styles.closeBtn} onClick={handleClose}><IconX size={14} /></button>
        </div>

        <div className={styles.contentBody}>
          {isSearching ? (
            matchedPanels.length === 0 ? (
              <div className={styles.emptySearch}>No settings match "{search}"</div>
            ) : (
              <div className={styles.searchGroup}>
                <div className={styles.searchGroupTitle}>Categories</div>
                {matchedPanels.map((panel) => {
                  const Icon = panel.icon;
                  return (
                    <button
                      key={panel.id}
                      type="button"
                      className={styles.searchResult}
                      onClick={() => { setSelected(panel.id); setSearch(''); }}
                    >
                      <Icon size={18} />
                      <span className={styles.searchResultCopy}>
                        <span className={styles.searchResultTitle}>{panel.label}</span>
                        <span className={styles.searchResultDescription}>{panel.description}</span>
                      </span>
                    </button>
                  );
                })}
              </div>
            )
          ) : ['general', 'sidebar', 'controls', 'preview', 'notifications', 'autoimport', 'subscriptions'].includes(activePanel.id) ? (
            <>
              <PreferencePanel
                panel={activePanel.id as 'general' | 'sidebar' | 'controls' | 'preview' | 'notifications' | 'autoimport' | 'subscriptions'}
                onDirty={markDirty}
                appSettings={pendingAppSettings}
                setAppSettings={setPendingAppSettings}
                prefs={pendingViewPrefs}
                setPrefs={setPendingViewPrefs}
                audioVisualization={pendingAudioVisualization}
                setAudioVisualization={setPendingAudioVisualization}
              />
            </>
          ) : activePanel.id === 'shortcuts' ? (
            <div className={styles.shortcutsContent}>
              <div className={styles.panelContent}>
                <KeyboardPresetRow
                  preset={pendingKeyboardPreset}
                  onChange={(preset) => {
                    setPendingKeyboardPreset(preset);
                    setKeyboardPreset(preset, false);
                    markDirty();
                  }}
                />
              </div>
              <ShortcutsPanel onDirty={markDirty} />
            </div>
          ) : activePanel.id === 'aitagging' ? (
            <AiTaggingPanel
              settings={pendingAppSettings}
              onSettingsChange={(patch) => {
                setPendingAppSettings((current) => current ? { ...current, ...patch } : current);
                markDirty();
              }}
            />
          ) : null}
        </div>

        {/* ── Footer — always visible ── */}
        <div className={styles.footer}>
          <span className={styles.saveStatus} role="status">{saveError}</span>
          <button
            className={styles.footerBtnPrimary}
            onClick={() => void handleSave(true)}
            disabled={isSaving}
            title="Save settings and close Preferences"
          >
            {isSaving ? 'Saving…' : 'Save & Close'}
          </button>
          <button
            className={styles.footerBtn}
            onClick={() => void handleSave(false)}
            disabled={isSaving}
            title="Save settings and keep Preferences open"
          >
            Apply
          </button>
        </div>
      </div>

    </div>
  );
}
