
import { t } from '../../i18n';/**
 * Centralized shortcut registry — single source of truth for all keyboard shortcuts.
 *
 * EU keyboard notes: keys2 provides alternatives for keys that require AltGr on
 * German (QWERTZ), French (AZERTY), and Nordic layouts. Backtick, backslash,
 * and square brackets are particularly problematic.
 */

export interface ShortcutDef {
  id: string;
  label: string;
  description?: string;
  group: string;
  keys: string;
  keys2?: string;
}

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

export const SHORTCUT_DEFS: ShortcutDef[] = [
  // ── Navigation ──
  { id: 'nav.allActive', label: t("All Active"),       group: t("Navigation"), keys: 'Mod+1' },
  { id: 'nav.inbox',     label: t("Inbox"),            group: t("Navigation"), keys: 'Mod+2' },
  { id: 'nav.untagged',  label: t("Untagged"),         group: t("Navigation"), keys: 'Mod+3' },
  { id: 'nav.trash',     label: t("Trash"),            group: t("Navigation"), keys: 'Mod+4' },
  { id: 'nav.search',    label: t("Search"),           group: t("Navigation"), keys: 'Mod+F' },
  { id: 'nav.commandPalette', label: t("Command Palette"), group: t("Navigation"), keys: 'Mod+K',            description: t("Open command palette") },
  { id: 'nav.goToFolder',     label: t("Go to Folder"),    group: t("Navigation"), keys: 'Mod+J',            description: t("Quick-jump to a folder or smart folder") },
  { id: 'nav.back',      label: t("Go Back"),          group: t("Navigation"), keys: 'Alt+ArrowLeft',  keys2: 'Mod+[' },
  { id: 'nav.forward',   label: t("Go Forward"),       group: t("Navigation"), keys: 'Alt+ArrowRight', keys2: 'Mod+]' },

  // ── File ──
  { id: 'file.import',             label: t("Import Files"),         group: t("File"), keys: 'Mod+I' },
  { id: 'file.export',             label: t("Export Originals"),     group: t("File"), keys: 'Mod+E' },
  { id: 'file.exportAs',           label: t("Export As..."),         group: t("File"), keys: 'Mod+Shift+E' },
  { id: 'file.settings',           label: t("Settings"),             group: t("File"), keys: 'Mod+,' },
  { id: 'file.delete',             label: t("Delete"),               group: t("File"), keys: 'Mod+Backspace', description: t("Context-dependent: trash in active scope, permanent delete in trash scope") },
  { id: 'file.restore',            label: t("Restore from Trash"),   group: t("File"), keys: 'Mod+Shift+Backspace' },
  { id: 'file.newFolder',          label: t("New Folder"),           group: t("File"), keys: 'Mod+Shift+N',     description: t("Create a new folder in the sidebar") },
  { id: 'file.newSubfolder',       label: t("New Subfolder"),        group: t("File"), keys: 'Alt+N',           description: t("Create a subfolder under the current folder") },
  { id: 'file.newSmartFolder',     label: t("New Smart Folder"),     group: t("File"), keys: 'Mod+Shift+Alt+N', description: t("Create a new smart folder") },
  { id: 'folder.autoTags',         label: t("Set Folder Auto Tags"), group: t("File"), keys: 'Mod+Shift+R',     description: t("Set tags applied when media enters the selected folder") },
  { id: 'file.addToFolder',        label: t("Add to Folder..."),     group: t("File"), keys: 'Mod+Shift+J',    description: t("Open folder picker to add selected files") },
  { id: 'file.addToLastFolder',    label: t("Add to Last Folder"),   group: t("File"), keys: 'Shift+D',        description: t("Add selected files to the last used folder") },
  { id: 'file.removeFromFolder',   label: t("Remove from Folder"),   group: t("File"), keys: 'Mod+Shift+Backspace' },
  { id: 'file.regenerateThumbnail', label: t("Regenerate Thumbnail"), group: t("File"), keys: 'Mod+Shift+T',   description: t("Regenerate thumbnails for selected files") },
  { id: 'file.openDefaultApp',     label: t("Open with Default App"), group: t("File"), keys: 'Shift+Enter' },
  { id: 'file.revealInFolder',     label: t("Reveal in Folder"),     group: t("File"), keys: 'Mod+Enter' },
  { id: 'file.openNewWindow',      label: t("Open in New Window"),   group: t("File"), keys: 'Mod+O' },

  // ── Edit ──
  { id: 'organize.addTag',    label: t("Add Tags"),       group: t("Edit"), keys: 'T',              description: t("Open tag panel for selected images") },
  { id: 'organize.addFolder', label: t("Add to Folders"),  group: t("Edit"), keys: 'F',             description: t("Open folder picker for selected images") },
  { id: 'organize.autoTag',   label: t("Auto-Tag"),        group: t("Edit"), keys: 'Mod+Shift+A',  description: t("Open AI auto-tagger for selected images") },
  { id: 'edit.undo',          label: t("Undo"),            group: t("Edit"), keys: 'Mod+Z' },
  { id: 'edit.redo',          label: t("Redo"),            group: t("Edit"), keys: 'Mod+Shift+Z' },
  { id: 'edit.selectAll',     label: t("Select All"),      group: t("Edit"), keys: 'Mod+A' },
  { id: 'edit.deselectAll',   label: t("Deselect All"),    group: t("Edit"), keys: 'Escape' },
  { id: 'edit.rename',        label: t("Rename"),          group: t("Edit"), keys: 'Ctrl+R',        description: t("Rename selected file") },
  { id: 'edit.copy',          label: t("Copy"),            group: t("Edit"), keys: 'Mod+C' },
  { id: 'edit.copyFilePath',  label: t("Copy File Path"),  group: t("Edit"), keys: 'Mod+Alt+C' },
  { id: 'edit.copyTags',      label: t("Copy Tags"),       group: t("Edit"), keys: 'Mod+Shift+C' },
  { id: 'edit.pasteTags',     label: t("Paste Tags"),      group: t("Edit"), keys: 'Mod+Shift+V' },
  { id: 'edit.pasteImport',   label: t("Paste Import"),     group: t("Edit"), keys: 'Mod+V' },

  // ── Groups ──
  { id: 'group.removeMembers', label: t("Remove from Group"), group: t("Groups"), keys: 'Delete', keys2: 'Backspace' },

  // ── Inbox ──
  { id: 'inbox.accept', label: t("Accept"), group: t("Inbox"), keys: 'Z', description: t("Accept inbox item (set to active)") },
  { id: 'inbox.reject', label: t("Reject"), group: t("Inbox"), keys: 'X', description: t("Reject inbox item (move to trash)") },

  // ── View ──
  { id: 'view.detailView',       label: t("Media View"),       group: t("View"), keys: 'Enter',                         description: t("Open selected image in media view") },
  { id: 'view.quicklook',        label: t("Quick Look"),       group: t("View"), keys: 'Space',                         description: t("Preview selected image") },
  { id: 'view.fitWindow',        label: t("Fit to Window"),    group: t("View"), keys: '`',         keys2: 'Shift+F',   description: t("EU: Shift+F (backtick inaccessible on DE/FR/Nordic)") },
  { id: 'view.actualSize',       label: t("Actual Size"),      group: t("View"), keys: 'Mod+0' },
  { id: 'view.zoomIn',           label: t("Zoom In"),          group: t("View"), keys: '+',         keys2: '=' },
  { id: 'view.zoomOut',          label: t("Zoom Out"),         group: t("View"), keys: '-' },
  { id: 'view.grayscale',        label: t("Toggle Grayscale"), group: t("View"), keys: 'Mod+Alt+G', description: t("Toggle grayscale preview mode") },
  { id: 'view.slideshow',        label: t("Slideshow"),        group: t("View"), keys: 'F5',        description: t("Start slideshow presentation mode") },
  { id: 'view.prevImage',        label: t("Previous Image"),   group: t("View"), keys: 'ArrowLeft', keys2: 'A' },
  { id: 'view.nextImage',        label: t("Next Image"),       group: t("View"), keys: 'ArrowRight', keys2: 'D' },
  { id: 'view.closeDetail',      label: t("Close Media View"), group: t("View"), keys: 'Escape' },
  { id: 'view.alwaysOnTop',      label: t("Always on Top"),    group: t("View"), keys: 'Shift+T',    description: t("Toggle window always on top") },
  { id: 'view.navigator',        label: t("Toggle Navigator"), group: t("View"), keys: 'Mod+Alt+8',  description: t("Toggle navigator overlay when zoomed") },
  { id: 'view.toggleSidebar',    label: t("Toggle Sidebar"),   group: t("View"), keys: 'Mod+Alt+1' },
  { id: 'view.toggleInspector',  label: t("Toggle Inspector"), group: t("View"), keys: 'Mod+Alt+2' },
  { id: 'view.toggleBothPanels', label: t("Toggle Panels"),    group: t("View"), keys: 'Tab' },
  { id: 'view.layoutGrid',       label: t("Grid Layout"),      group: t("View"), keys: 'Alt+1' },
  { id: 'view.layoutWaterfall',  label: t("Waterfall Layout"),  group: t("View"), keys: 'Alt+2' },
  { id: 'view.layoutJustified',  label: t("Justified Layout"),  group: t("View"), keys: 'Alt+3' },
  { id: 'view.toggleTileName',   label: t("Toggle Tile Name"),  group: t("View"), keys: 'Mod+Alt+4' },
  { id: 'view.toggleTileMetadata', label: t("Toggle Tile Info"), group: t("View"), keys: 'Mod+Alt+5', description: t("Show or hide resolution and extension on tiles") },
  { id: 'view.toggleLogs',       label: t("Toggle Logs"),       group: t("View"), keys: 'Mod+L',     description: t("Show or hide the log viewer panel") },
  { id: 'document.previousPage', label: t("Previous Page"),     group: t("View"), keys: 'J',         description: t("Open the previous page in a document") },
  { id: 'document.nextPage',     label: t("Next Page"),         group: t("View"), keys: 'L',         description: t("Open the next page in a document") },

  // ── Grid navigation ──
  { id: 'grid.moveLeft',  label: t("Grid: Move Left"),  group: t("Navigation"), keys: 'ArrowLeft',  keys2: 'A' },
  { id: 'grid.moveRight', label: t("Grid: Move Right"), group: t("Navigation"), keys: 'ArrowRight', keys2: 'D' },
  { id: 'grid.moveUp',    label: t("Grid: Move Up"),    group: t("Navigation"), keys: 'ArrowUp',    keys2: 'W' },
  { id: 'grid.moveDown',  label: t("Grid: Move Down"),  group: t("Navigation"), keys: 'ArrowDown',  keys2: 'S' },
  { id: 'grid.first',     label: t("First Image"),      group: t("Navigation"), keys: 'Home' },
  { id: 'grid.last',      label: t("Last Image"),       group: t("Navigation"), keys: 'End' },
  { id: 'grid.pageUp',    label: t("Page Up"),           group: t("Navigation"), keys: 'PageUp',     description: t("Jump up by one screenful") },
  { id: 'grid.pageDown',  label: t("Page Down"),         group: t("Navigation"), keys: 'PageDown',   description: t("Jump down by one screenful") },

  // ── Rating ──
  { id: 'rate.0', label: t("Clear Rating"), group: t("Rating"), keys: '0', description: t("Remove rating from selected images") },
  { id: 'rate.1', label: t("Rate 1 Star"),  group: t("Rating"), keys: '1', description: t("Rate selected images 1 star") },
  { id: 'rate.2', label: t("Rate 2 Stars"), group: t("Rating"), keys: '2', description: t("Rate selected images 2 stars") },
  { id: 'rate.3', label: t("Rate 3 Stars"), group: t("Rating"), keys: '3', description: t("Rate selected images 3 stars") },
  { id: 'rate.4', label: t("Rate 4 Stars"), group: t("Rating"), keys: '4', description: t("Rate selected images 4 stars") },
  { id: 'rate.5', label: t("Rate 5 Stars"), group: t("Rating"), keys: '5', description: t("Rate selected images 5 stars") },

  // ── Video ──
  { id: 'video.togglePlay',   label: t("Toggle Play/Pause"), group: t("Video"), keys: 'K',            description: t("Play or pause video; Space remains the Quick Look toggle") },
  { id: 'video.seekBackward', label: t("Seek Backward"),     group: t("Video"), keys: 'J',         description: t("Seek backward 5 seconds") },
  { id: 'video.seekForward',  label: t("Seek Forward"),      group: t("Video"), keys: 'L',         description: t("Seek forward 5 seconds") },
  { id: 'video.volumeUp',     label: t("Volume Up"),         group: t("Video"), keys: 'ArrowUp',   description: t("Increase volume") },
  { id: 'video.volumeDown',   label: t("Volume Down"),       group: t("Video"), keys: 'ArrowDown', description: t("Decrease volume") },
  { id: 'video.toggleMute',   label: t("Toggle Mute"),       group: t("Video"), keys: 'M',         description: t("Mute or unmute video") },
  { id: 'video.toggleLoop',   label: t("Toggle Loop"),       group: t("Video"), keys: 'O',         description: t("Toggle loop playback") },
  { id: 'video.rateIncrease', label: t("Speed Up"),          group: t("Video"), keys: ']',  keys2: 'Shift+.',  description: t("Increase playback speed") },
  { id: 'video.rateDecrease', label: t("Slow Down"),         group: t("Video"), keys: '[',  keys2: 'Shift+,',  description: t("Decrease playback speed") },
  { id: 'video.rateReset',    label: t("Reset Speed"),       group: t("Video"), keys: '\\', keys2: 'Shift+R', description: t("Reset playback speed to 1x") },
  { id: 'video.fullscreen',   label: t("Toggle Fullscreen"), group: t("Video"), keys: 'F',         description: t("Enter or leave fullscreen playback") },

  // ── Duplicates ──
  { id: 'dup.smartMerge',   label: t("Smart Merge"),   group: t("Duplicates"), keys: 'S',          description: t("Auto-merge keeping the better file") },
  { id: 'dup.keepLeft',     label: t("Keep Left"),     group: t("Duplicates"), keys: 'Z',          description: t("Keep the left file, delete right") },
  { id: 'dup.keepRight',    label: t("Keep Right"),    group: t("Duplicates"), keys: 'X',          description: t("Keep the right file, delete left") },
  { id: 'dup.keepBoth',     label: t("Keep Both"),     group: t("Duplicates"), keys: 'B',          description: t("Keep both files and dismiss the pair") },
  { id: 'dup.notDuplicate', label: t("Not Duplicate"), group: t("Duplicates"), keys: 'N',          description: t("Mark pair as not duplicate") },
  { id: 'dup.fitToWindow',  label: t("Fit to Window"), group: t("Duplicates"), keys: 'F',          description: t("Reset zoom to fit images in view") },
  { id: 'dup.prevPair',     label: t("Previous Pair"), group: t("Duplicates"), keys: 'ArrowLeft',  description: t("Go to previous duplicate pair") },
  { id: 'dup.nextPair',     label: t("Next Pair"),     group: t("Duplicates"), keys: 'ArrowRight', description: t("Go to next duplicate pair") },
];

// ── Keyboard Presets ──
// US layout: defaults (keys is primary, keys2 is secondary/EU fallback)
// EU layout: swaps keys/keys2 for shortcuts that use AltGr-dependent characters

export type KeyboardPreset = 'us' | 'eu';

const STORAGE_KEY = 'picto-keyboard-preset';
const OVERRIDES_STORAGE_KEY = 'picto-shortcut-overrides';
export const SHORTCUT_STATE_CHANGED_EVENT = 'picto:shortcut-state-changed';

let activePreset: KeyboardPreset = (localStorage.getItem(STORAGE_KEY) as KeyboardPreset) || 'us';

export interface ShortcutBindingOverride {
  keys?: string;
  keys2?: string;
}

function loadOverrides(): Record<string, ShortcutBindingOverride> {
  try {
    const value = JSON.parse(localStorage.getItem(OVERRIDES_STORAGE_KEY) ?? '{}') as unknown;
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
    return Object.fromEntries(Object.entries(value).flatMap(([id, override]) => {
      if (!override || typeof override !== 'object' || Array.isArray(override)) return [];
      const candidate = override as Record<string, unknown>;
      const keys = typeof candidate.keys === 'string' ? candidate.keys : undefined;
      const keys2 = typeof candidate.keys2 === 'string' ? candidate.keys2 : undefined;
      return keys === undefined && keys2 === undefined ? [] : [[id, { keys, keys2 }]];
    }));
  } catch {
    return {};
  }
}

let shortcutOverrides = loadOverrides();

/** For EU-problematic shortcuts, swap keys and keys2 so the EU-friendly binding is primary. */
const EU_SWAP_IDS = new Set([
  'view.fitWindow',      // ` → Shift+F
  'video.rateIncrease',  // ] → Shift+.
  'video.rateDecrease',  // [ → Shift+,
  'video.rateReset',     // \ → Shift+R
]);

const DEFAULT_KEY_BINDINGS = new Map(
  SHORTCUT_DEFS
    .filter((def) => EU_SWAP_IDS.has(def.id) && def.keys2)
    .map((def) => [def.id, { keys: def.keys, keys2: def.keys2! }]),
);

export function getKeyboardPreset(): KeyboardPreset { return activePreset; }

export function setKeyboardPreset(preset: KeyboardPreset, persist = true): void {
  activePreset = preset;
  if (persist) localStorage.setItem(STORAGE_KEY, preset);
  // Always derive from the immutable US defaults so switching back is reliable.
  for (const def of SHORTCUT_DEFS) {
    const defaults = DEFAULT_KEY_BINDINGS.get(def.id);
    if (!defaults) continue;
    def.keys = preset === 'eu' ? defaults.keys2 : defaults.keys;
    def.keys2 = preset === 'eu' ? defaults.keys : defaults.keys2;
  }
}

/** Reload bindings written by another Picto window. */
export function reloadShortcutStateFromStorage(): void {
  const storedPreset = localStorage.getItem(STORAGE_KEY);
  setKeyboardPreset(storedPreset === 'eu' ? 'eu' : 'us', false);
  shortcutOverrides = loadOverrides();
}

function announceShortcutStateChanged(): void {
  if (typeof window !== 'undefined') window.dispatchEvent(new Event(SHORTCUT_STATE_CHANGED_EVENT));
}

// Apply stored preset on load
if (activePreset === 'eu') setKeyboardPreset('eu', false);

if (typeof window !== 'undefined') {
  window.addEventListener('storage', (event) => {
    if (event.storageArea !== localStorage) return;
    if (event.key !== STORAGE_KEY && event.key !== OVERRIDES_STORAGE_KEY && event.key !== null) return;
    reloadShortcutStateFromStorage();
    announceShortcutStateChanged();
  });
}

// ── Helpers ──

export interface ShortcutGroup { name: string; items: ShortcutDef[]; }

function resolveShortcut(def: ShortcutDef): ShortcutDef {
  return { ...def, ...shortcutOverrides[def.id] };
}

export function getShortcutGroups(): ShortcutGroup[] {
  const map = new Map<string, ShortcutDef[]>();
  for (const source of SHORTCUT_DEFS) {
    const def = resolveShortcut(source);
    let list = map.get(def.group);
    if (!list) { list = []; map.set(def.group, list); }
    list.push(def);
  }
  const order = [
    t('Navigation'), t('File'), t('Edit'), t('Groups'), t('Rating'),
    t('View'), t('Inbox'), t('Video'), t('Duplicates'),
  ];
  return order.filter((g) => map.has(g)).map((g) => ({ name: g, items: map.get(g)! }));
}

const MAC_SYMBOLS: Record<string, string> = {
  Mod: '⌘', Shift: '⇧', Alt: '⌥', Ctrl: '⌃',
  ArrowLeft: '←', ArrowRight: '→', ArrowUp: '↑', ArrowDown: '↓',
  Enter: '↩', Escape: 'Esc', Backspace: '⌫', Delete: '⌦',
  Space: '␣', Tab: '⇥', PageUp: 'PgUp', PageDown: 'PgDn',
};

const WIN_LABELS: Record<string, string> = {
  Mod: 'Ctrl', ArrowLeft: '←', ArrowRight: '→', ArrowUp: '↑', ArrowDown: '↓',
  Backspace: 'Backspace', Delete: 'Del', Escape: 'Esc', Enter: 'Enter',
  Space: 'Space', Tab: 'Tab', PageUp: 'PgUp', PageDown: 'PgDn',
};

export function formatKeysDisplay(keys: string): string {
  return formatKeysAsArray(keys).join(isMac ? '' : '+');
}

export function formatKeysAsArray(keys: string): string[] {
  if (keys === '+') return ['+'];
  let base = keys;
  let trailingPlus = false;
  if (keys.endsWith('++')) { base = keys.slice(0, -2); trailingPlus = true; }
  const parts = base.split('+').filter(Boolean);
  if (trailingPlus) parts.push('+');
  const lookup = isMac ? MAC_SYMBOLS : WIN_LABELS;
  return parts.map((p) => lookup[p] ?? p);
}

export function getShortcut(id: string): ShortcutDef | undefined {
  const def = SHORTCUT_DEFS.find((candidate) => candidate.id === id);
  return def ? resolveShortcut(def) : undefined;
}

export function setShortcutBinding(id: string, slot: 'keys' | 'keys2', value: string, persist = true): void {
  if (!SHORTCUT_DEFS.some((def) => def.id === id)) return;
  shortcutOverrides = {
    ...shortcutOverrides,
    [id]: { ...shortcutOverrides[id], [slot]: value },
  };
  if (persist) localStorage.setItem(OVERRIDES_STORAGE_KEY, JSON.stringify(shortcutOverrides));
}

export function findShortcutConflict(id: string, value: string): ShortcutDef | undefined {
  if (!value) return undefined;
  return SHORTCUT_DEFS
    .map(resolveShortcut)
    .find((def) => def.id !== id && (def.keys === value || def.keys2 === value));
}

export function getShortcutOverrides(): Readonly<Record<string, ShortcutBindingOverride>> {
  return structuredClone(shortcutOverrides);
}

export function replaceShortcutOverrides(
  overrides: Readonly<Record<string, ShortcutBindingOverride>>,
  persist = true,
): void {
  shortcutOverrides = structuredClone(overrides);
  if (persist) localStorage.setItem(OVERRIDES_STORAGE_KEY, JSON.stringify(shortcutOverrides));
}

export function persistShortcutState(): void {
  localStorage.setItem(STORAGE_KEY, activePreset);
  localStorage.setItem(OVERRIDES_STORAGE_KEY, JSON.stringify(shortcutOverrides));
  announceShortcutStateChanged();
}

const APPLICATION_MENU_SHORTCUT_IDS = [
  'file.settings',
  'file.import',
  'file.export',
  'file.exportAs',
  'edit.undo',
  'edit.redo',
  'nav.allActive',
  'nav.inbox',
  'nav.untagged',
  'nav.trash',
  'view.toggleLogs',
] as const;

export function getApplicationMenuShortcutBindings(): Record<string, string> {
  return Object.fromEntries(APPLICATION_MENU_SHORTCUT_IDS.map((id) => [id, getShortcut(id)?.keys ?? '']));
}

export interface ShortcutMatchOptions {
  /** Range selection adds Shift to a configurable movement binding. */
  allowExtraShift?: boolean;
}

export function matchesShortcut(e: KeyboardEvent, keys: string, options: ShortcutMatchOptions = {}): boolean {
  if (!keys) return false;
  let base = keys;
  let trailingPlus = false;
  if (keys === '+') { base = ''; trailingPlus = true; }
  else if (keys.endsWith('++')) { base = keys.slice(0, -2); trailingPlus = true; }
  const parts = base ? base.split('+').filter(Boolean) : [];
  if (trailingPlus) parts.push('+');

  const modifiers = new Set<string>();
  let targetKey = '';
  for (const p of parts) {
    if (p === 'Mod' || p === 'Ctrl' || p === 'Alt' || p === 'Shift') modifiers.add(p);
    else targetKey = p;
  }

  const wantMod = modifiers.has('Mod');
  const hasMod = isMac ? e.metaKey : e.ctrlKey;
  if (wantMod !== hasMod) return false;
  const wantCtrl = modifiers.has('Ctrl');
  const hasCtrl = isMac ? e.ctrlKey : false;
  if (wantCtrl !== hasCtrl) return false;
  if (modifiers.has('Alt') !== e.altKey) return false;
  const wantShift = modifiers.has('Shift');
  const hasAllowedExtraShift = options.allowExtraShift && !wantShift && e.shiftKey;
  if (wantShift !== e.shiftKey && !hasAllowedExtraShift) return false;
  if (!wantMod && !wantCtrl && !modifiers.has('Alt') && !modifiers.has('Shift')) {
    if (e.metaKey || e.ctrlKey || e.altKey || (e.shiftKey && !hasAllowedExtraShift)) return false;
  }

  let normalizedKey = e.key;
  if (e.key === ' ') normalizedKey = 'Space';
  else if (e.key.length === 1) normalizedKey = e.key.toUpperCase();
  return normalizedKey === targetKey;
}

export function matchesShortcutDef(e: KeyboardEvent, def: ShortcutDef, options: ShortcutMatchOptions = {}): boolean {
  if (matchesShortcut(e, def.keys, options)) return true;
  if (def.keys2 && matchesShortcut(e, def.keys2, options)) return true;
  return false;
}
