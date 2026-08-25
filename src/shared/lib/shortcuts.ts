/**
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
  { id: 'nav.allActive', label: 'All Active',       group: 'Navigation', keys: 'Mod+1' },
  { id: 'nav.inbox',     label: 'Inbox',            group: 'Navigation', keys: 'Mod+2' },
  { id: 'nav.untagged',  label: 'Untagged',         group: 'Navigation', keys: 'Mod+3' },
  { id: 'nav.trash',     label: 'Trash',            group: 'Navigation', keys: 'Mod+4' },
  { id: 'nav.search',    label: 'Search',           group: 'Navigation', keys: 'Mod+F' },
  { id: 'nav.commandPalette', label: 'Command Palette', group: 'Navigation', keys: 'Mod+K',            description: 'Open command palette' },
  { id: 'nav.goToFolder',     label: 'Go to Folder',    group: 'Navigation', keys: 'Mod+J',            description: 'Quick-jump to a folder or smart folder' },
  { id: 'nav.back',      label: 'Go Back',          group: 'Navigation', keys: 'Alt+ArrowLeft',  keys2: 'Mod+[' },
  { id: 'nav.forward',   label: 'Go Forward',       group: 'Navigation', keys: 'Alt+ArrowRight', keys2: 'Mod+]' },

  // ── File ──
  { id: 'file.import',             label: 'Import Files',         group: 'File', keys: 'Mod+I' },
  { id: 'file.export',             label: 'Export Originals',     group: 'File', keys: 'Mod+E' },
  { id: 'file.exportAs',           label: 'Export As...',         group: 'File', keys: 'Mod+Shift+E' },
  { id: 'file.settings',           label: 'Settings',             group: 'File', keys: 'Mod+,' },
  { id: 'file.delete',             label: 'Delete',               group: 'File', keys: 'Mod+Backspace', description: 'Context-dependent: trash in active scope, permanent delete in trash scope' },
  { id: 'file.restore',            label: 'Restore from Trash',   group: 'File', keys: 'Mod+Shift+Backspace' },
  { id: 'file.newFolder',          label: 'New Folder',           group: 'File', keys: 'Mod+Shift+N',     description: 'Create a new folder in the sidebar' },
  { id: 'file.newSubfolder',       label: 'New Subfolder',        group: 'File', keys: 'Alt+N',           description: 'Create a subfolder under the current folder' },
  { id: 'file.newSmartFolder',     label: 'New Smart Folder',     group: 'File', keys: 'Mod+Shift+Alt+N', description: 'Create a new smart folder' },
  { id: 'folder.autoTags',         label: 'Set Folder Auto Tags', group: 'File', keys: 'Mod+Shift+R',     description: 'Set tags applied when media enters the selected folder' },
  { id: 'file.addToFolder',        label: 'Add to Folder...',     group: 'File', keys: 'Mod+Shift+J',    description: 'Open folder picker to add selected files' },
  { id: 'file.addToLastFolder',    label: 'Add to Last Folder',   group: 'File', keys: 'Shift+D',        description: 'Add selected files to the last used folder' },
  { id: 'file.removeFromFolder',   label: 'Remove from Folder',   group: 'File', keys: 'Mod+Shift+Backspace' },
  { id: 'file.regenerateThumbnail', label: 'Regenerate Thumbnail', group: 'File', keys: 'Mod+Shift+T',   description: 'Regenerate thumbnails for selected files' },
  { id: 'file.openDefaultApp',     label: 'Open with Default App', group: 'File', keys: 'Shift+Enter' },
  { id: 'file.revealInFolder',     label: 'Reveal in Folder',     group: 'File', keys: 'Mod+Enter' },
  { id: 'file.openNewWindow',      label: 'Open in New Window',   group: 'File', keys: 'Mod+O' },

  // ── Edit ──
  { id: 'organize.addTag',    label: 'Add Tags',       group: 'Edit', keys: 'T',              description: 'Open tag panel for selected images' },
  { id: 'organize.addFolder', label: 'Add to Folders',  group: 'Edit', keys: 'F',             description: 'Open folder picker for selected images' },
  { id: 'organize.autoTag',   label: 'Auto-Tag',        group: 'Edit', keys: 'Mod+Shift+A',  description: 'Open AI auto-tagger for selected images' },
  { id: 'edit.undo',          label: 'Undo',            group: 'Edit', keys: 'Mod+Z' },
  { id: 'edit.redo',          label: 'Redo',            group: 'Edit', keys: 'Mod+Shift+Z' },
  { id: 'edit.selectAll',     label: 'Select All',      group: 'Edit', keys: 'Mod+A' },
  { id: 'edit.deselectAll',   label: 'Deselect All',    group: 'Edit', keys: 'Escape' },
  { id: 'edit.rename',        label: 'Rename',          group: 'Edit', keys: 'Ctrl+R',        description: 'Rename selected file' },
  { id: 'edit.copy',          label: 'Copy',            group: 'Edit', keys: 'Mod+C' },
  { id: 'edit.copyFilePath',  label: 'Copy File Path',  group: 'Edit', keys: 'Mod+Alt+C' },
  { id: 'edit.copyTags',      label: 'Copy Tags',       group: 'Edit', keys: 'Mod+Shift+C' },
  { id: 'edit.pasteTags',     label: 'Paste Tags',      group: 'Edit', keys: 'Mod+Shift+V' },
  { id: 'edit.pasteImport',   label: 'Paste Import',     group: 'Edit', keys: 'Mod+V' },

  // ── Inbox ──
  { id: 'inbox.accept', label: 'Accept', group: 'Inbox', keys: 'Enter',     description: 'Accept inbox image (set to active)' },
  { id: 'inbox.reject', label: 'Reject', group: 'Inbox', keys: 'Backspace', description: 'Reject inbox image (move to trash)' },

  // ── View ──
  { id: 'view.detailView',       label: 'Media View',       group: 'View', keys: 'Enter',                         description: 'Open selected image in media view' },
  { id: 'view.quicklook',        label: 'Quick Look',       group: 'View', keys: 'Space',                         description: 'Preview selected image' },
  { id: 'view.fitWindow',        label: 'Fit to Window',    group: 'View', keys: '`',         keys2: 'Shift+F',   description: 'EU: Shift+F (backtick inaccessible on DE/FR/Nordic)' },
  { id: 'view.actualSize',       label: 'Actual Size',      group: 'View', keys: 'Mod+0' },
  { id: 'view.zoomIn',           label: 'Zoom In',          group: 'View', keys: '+',         keys2: '=' },
  { id: 'view.zoomOut',          label: 'Zoom Out',         group: 'View', keys: '-' },
  { id: 'view.grayscale',        label: 'Toggle Grayscale', group: 'View', keys: 'Mod+Alt+G', description: 'Toggle grayscale preview mode' },
  { id: 'view.slideshow',        label: 'Slideshow',        group: 'View', keys: 'F5',        description: 'Start slideshow presentation mode' },
  { id: 'view.prevImage',        label: 'Previous Image',   group: 'View', keys: 'ArrowLeft', keys2: 'A' },
  { id: 'view.nextImage',        label: 'Next Image',       group: 'View', keys: 'ArrowRight', keys2: 'D' },
  { id: 'view.closeDetail',      label: 'Close Media View', group: 'View', keys: 'Escape' },
  { id: 'view.alwaysOnTop',      label: 'Always on Top',    group: 'View', keys: 'Shift+T',    description: 'Toggle window always on top' },
  { id: 'view.navigator',        label: 'Toggle Navigator', group: 'View', keys: 'Mod+Alt+8',  description: 'Toggle navigator overlay when zoomed' },
  { id: 'view.toggleSidebar',    label: 'Toggle Sidebar',   group: 'View', keys: 'Mod+Alt+1' },
  { id: 'view.toggleInspector',  label: 'Toggle Inspector', group: 'View', keys: 'Mod+Alt+2' },
  { id: 'view.toggleBothPanels', label: 'Toggle Panels',    group: 'View', keys: 'Tab' },
  { id: 'view.layoutGrid',       label: 'Grid Layout',      group: 'View', keys: 'Alt+1' },
  { id: 'view.layoutWaterfall',  label: 'Waterfall Layout',  group: 'View', keys: 'Alt+2' },
  { id: 'view.layoutJustified',  label: 'Justified Layout',  group: 'View', keys: 'Alt+3' },
  { id: 'view.toggleTileName',   label: 'Toggle Tile Name',  group: 'View', keys: 'Mod+Alt+4' },
  { id: 'view.toggleTileMetadata', label: 'Toggle Tile Info', group: 'View', keys: 'Mod+Alt+5', description: 'Show or hide resolution and extension on tiles' },
  { id: 'view.toggleLogs',       label: 'Toggle Logs',       group: 'View', keys: 'Mod+L',     description: 'Show or hide the log viewer panel' },

  // ── Grid navigation ──
  { id: 'grid.moveLeft',  label: 'Grid: Move Left',  group: 'Navigation', keys: 'ArrowLeft',  keys2: 'A' },
  { id: 'grid.moveRight', label: 'Grid: Move Right', group: 'Navigation', keys: 'ArrowRight', keys2: 'D' },
  { id: 'grid.moveUp',    label: 'Grid: Move Up',    group: 'Navigation', keys: 'ArrowUp',    keys2: 'W' },
  { id: 'grid.moveDown',  label: 'Grid: Move Down',  group: 'Navigation', keys: 'ArrowDown',  keys2: 'S' },
  { id: 'grid.first',     label: 'First Image',      group: 'Navigation', keys: 'Home' },
  { id: 'grid.last',      label: 'Last Image',       group: 'Navigation', keys: 'End' },
  { id: 'grid.pageUp',    label: 'Page Up',           group: 'Navigation', keys: 'PageUp',     description: 'Jump up by one screenful' },
  { id: 'grid.pageDown',  label: 'Page Down',         group: 'Navigation', keys: 'PageDown',   description: 'Jump down by one screenful' },

  // ── Rating ──
  { id: 'rate.0', label: 'Clear Rating', group: 'Rating', keys: '0', description: 'Remove rating from selected images' },
  { id: 'rate.1', label: 'Rate 1 Star',  group: 'Rating', keys: '1', description: 'Rate selected images 1 star' },
  { id: 'rate.2', label: 'Rate 2 Stars', group: 'Rating', keys: '2', description: 'Rate selected images 2 stars' },
  { id: 'rate.3', label: 'Rate 3 Stars', group: 'Rating', keys: '3', description: 'Rate selected images 3 stars' },
  { id: 'rate.4', label: 'Rate 4 Stars', group: 'Rating', keys: '4', description: 'Rate selected images 4 stars' },
  { id: 'rate.5', label: 'Rate 5 Stars', group: 'Rating', keys: '5', description: 'Rate selected images 5 stars' },

  // ── Video ──
  { id: 'video.togglePlay',   label: 'Toggle Play/Pause', group: 'Video', keys: 'P', keys2: 'K', description: 'Play or pause video' },
  { id: 'video.seekBackward', label: 'Seek Backward',     group: 'Video', keys: 'J',         description: 'Seek backward 5 seconds' },
  { id: 'video.seekForward',  label: 'Seek Forward',      group: 'Video', keys: 'L',         description: 'Seek forward 5 seconds' },
  { id: 'video.volumeUp',     label: 'Volume Up',         group: 'Video', keys: 'ArrowUp',   description: 'Increase volume' },
  { id: 'video.volumeDown',   label: 'Volume Down',       group: 'Video', keys: 'ArrowDown', description: 'Decrease volume' },
  { id: 'video.toggleMute',   label: 'Toggle Mute',       group: 'Video', keys: 'M',         description: 'Mute or unmute video' },
  { id: 'video.toggleLoop',   label: 'Toggle Loop',       group: 'Video', keys: 'Shift+L',   description: 'Toggle loop playback' },
  { id: 'video.rateIncrease', label: 'Speed Up',          group: 'Video', keys: ']',  keys2: 'Shift+.',  description: 'Increase playback speed' },
  { id: 'video.rateDecrease', label: 'Slow Down',         group: 'Video', keys: '[',  keys2: 'Shift+,',  description: 'Decrease playback speed' },
  { id: 'video.rateReset',    label: 'Reset Speed',       group: 'Video', keys: 'Backspace', description: 'Reset playback speed to 1x' },

  // ── Duplicates ──
  { id: 'dup.smartMerge',   label: 'Smart Merge',   group: 'Duplicates', keys: 'S',          description: 'Auto-merge keeping the better file' },
  { id: 'dup.keepLeft',     label: 'Keep Left',     group: 'Duplicates', keys: 'L',          description: 'Keep the left file, delete right' },
  { id: 'dup.keepRight',    label: 'Keep Right',    group: 'Duplicates', keys: 'R',          description: 'Keep the right file, delete left' },
  { id: 'dup.notDuplicate', label: 'Not Duplicate', group: 'Duplicates', keys: 'N',          description: 'Mark pair as not duplicate' },
  { id: 'dup.fitToWindow',  label: 'Fit to Window', group: 'Duplicates', keys: 'F',          description: 'Reset zoom to fit images in view' },
  { id: 'dup.prevPair',     label: 'Previous Pair', group: 'Duplicates', keys: 'ArrowLeft',  description: 'Go to previous duplicate pair' },
  { id: 'dup.nextPair',     label: 'Next Pair',     group: 'Duplicates', keys: 'ArrowRight', description: 'Go to next duplicate pair' },
];

// ── Keyboard Presets ──
// US layout: defaults (keys is primary, keys2 is secondary/EU fallback)
// EU layout: swaps keys/keys2 for shortcuts that use AltGr-dependent characters

export type KeyboardPreset = 'us' | 'eu';

const STORAGE_KEY = 'picto-keyboard-preset';
const OVERRIDES_STORAGE_KEY = 'picto-shortcut-overrides';

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

// Apply stored preset on load
if (activePreset === 'eu') setKeyboardPreset('eu', false);

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
  const order = ['Navigation', 'File', 'Edit', 'Rating', 'View', 'Inbox', 'Video', 'Duplicates'];
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
}

export function matchesShortcut(e: KeyboardEvent, keys: string): boolean {
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
  if (modifiers.has('Shift') !== e.shiftKey) return false;
  if (!wantMod && !wantCtrl && !modifiers.has('Alt') && !modifiers.has('Shift')) {
    if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return false;
  }

  let normalizedKey = e.key;
  if (e.key === ' ') normalizedKey = 'Space';
  else if (e.key.length === 1) normalizedKey = e.key.toUpperCase();
  return normalizedKey === targetKey;
}

export function matchesShortcutDef(e: KeyboardEvent, def: ShortcutDef): boolean {
  if (matchesShortcut(e, def.keys)) return true;
  if (def.keys2 && matchesShortcut(e, def.keys2)) return true;
  return false;
}
