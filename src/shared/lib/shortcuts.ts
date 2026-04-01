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
  { id: 'nav.allActive', label: 'All Active',    group: 'Navigation', keys: 'Mod+1' },
  { id: 'nav.inbox',     label: 'Inbox',         group: 'Navigation', keys: 'Mod+2' },
  { id: 'nav.untagged',  label: 'Untagged',      group: 'Navigation', keys: 'Mod+3' },
  { id: 'nav.trash',     label: 'Trash',         group: 'Navigation', keys: 'Mod+4' },
  { id: 'nav.search',    label: 'Search',        group: 'Navigation', keys: 'Mod+F' },
  { id: 'nav.back',      label: 'Go Back',       group: 'Navigation', keys: 'Mod+[',  keys2: 'Mod+Alt+ArrowLeft' },
  { id: 'nav.forward',   label: 'Go Forward',    group: 'Navigation', keys: 'Mod+]',  keys2: 'Mod+Alt+ArrowRight' },

  // ── File ──
  { id: 'file.settings',            label: 'Settings',             group: 'File', keys: 'Mod+,' },
  { id: 'file.delete',              label: 'Move to Trash',        group: 'File', keys: 'Mod+Backspace', description: 'Context-dependent: trash in active scope, permanent delete in trash scope' },
  { id: 'file.restore',             label: 'Restore from Trash',   group: 'File', keys: 'Mod+Shift+Backspace' },
  { id: 'file.newFolder',           label: 'New Folder',           group: 'File', keys: 'Mod+Shift+N' },
  { id: 'file.addToFolder',         label: 'Add to Folder...',     group: 'File', keys: 'Mod+Shift+J' },
  { id: 'file.removeFromFolder',    label: 'Remove from Folder',   group: 'File', keys: 'Mod+Shift+Backspace' },
  { id: 'file.regenerateThumbnail', label: 'Regenerate Thumbnail', group: 'File', keys: 'Mod+Shift+T' },
  { id: 'file.openDefaultApp',     label: 'Open with Default App', group: 'File', keys: 'Shift+Enter' },
  { id: 'file.revealInFolder',     label: 'Reveal in Folder',     group: 'File', keys: 'Mod+Enter' },
  { id: 'file.openNewWindow',      label: 'Open in New Window',   group: 'File', keys: 'Mod+O' },

  // ── Edit ──
  { id: 'organize.addTag',    label: 'Add Tags',       group: 'Edit', keys: 'T',              description: 'Focus tag input in inspector' },
  { id: 'organize.addFolder', label: 'Add to Folders',  group: 'Edit', keys: 'Shift+F',       description: 'Open folder picker' },
  { id: 'edit.undo',          label: 'Undo',            group: 'Edit', keys: 'Mod+Z' },
  { id: 'edit.redo',          label: 'Redo',            group: 'Edit', keys: 'Mod+Shift+Z' },
  { id: 'edit.selectAll',     label: 'Select All',      group: 'Edit', keys: 'Mod+A' },
  { id: 'edit.deselectAll',   label: 'Deselect All',    group: 'Edit', keys: 'Escape' },
  { id: 'edit.rename',        label: 'Rename',          group: 'Edit', keys: 'Mod+R' },
  { id: 'edit.copy',          label: 'Copy',            group: 'Edit', keys: 'Mod+C' },
  { id: 'edit.copyFilePath',  label: 'Copy File Path',  group: 'Edit', keys: 'Mod+Alt+C' },
  { id: 'edit.copyTags',      label: 'Copy Tags',       group: 'Edit', keys: 'Mod+Shift+C' },
  { id: 'edit.pasteTags',     label: 'Paste Tags',      group: 'Edit', keys: 'Mod+Shift+V' },

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
  { id: 'view.prevImage',        label: 'Previous Image',   group: 'View', keys: 'ArrowLeft', keys2: 'A' },
  { id: 'view.nextImage',        label: 'Next Image',       group: 'View', keys: 'ArrowRight', keys2: 'D' },
  { id: 'view.closeDetail',      label: 'Close Media View', group: 'View', keys: 'Escape' },
  { id: 'view.toggleSidebar',    label: 'Toggle Sidebar',   group: 'View', keys: 'Mod+\\',    keys2: 'Mod+Shift+S', description: 'EU: Mod+Shift+S (backslash needs AltGr on EU)' },
  { id: 'view.toggleInspector',  label: 'Toggle Inspector', group: 'View', keys: 'Mod+Alt+2' },
  { id: 'view.toggleBothPanels', label: 'Toggle Panels',    group: 'View', keys: 'Tab' },
  { id: 'view.layoutGrid',       label: 'Grid Layout',      group: 'View', keys: 'Alt+1' },
  { id: 'view.layoutWaterfall',  label: 'Waterfall Layout',  group: 'View', keys: 'Alt+2' },
  { id: 'view.layoutJustified',  label: 'Justified Layout',  group: 'View', keys: 'Alt+3' },
  { id: 'view.toggleTileName',   label: 'Toggle Tile Name',  group: 'View', keys: 'Mod+Alt+4' },

  // ── Grid navigation ──
  { id: 'grid.moveLeft',  label: 'Grid: Move Left',  group: 'Navigation', keys: 'ArrowLeft',  keys2: 'A' },
  { id: 'grid.moveRight', label: 'Grid: Move Right', group: 'Navigation', keys: 'ArrowRight', keys2: 'D' },
  { id: 'grid.moveUp',    label: 'Grid: Move Up',    group: 'Navigation', keys: 'ArrowUp',    keys2: 'W' },
  { id: 'grid.moveDown',  label: 'Grid: Move Down',  group: 'Navigation', keys: 'ArrowDown',  keys2: 'S' },
  { id: 'grid.first',     label: 'First Image',      group: 'Navigation', keys: 'Home' },
  { id: 'grid.last',      label: 'Last Image',       group: 'Navigation', keys: 'End' },

  // ── Rating ──
  { id: 'rate.0', label: 'Clear Rating', group: 'Rating', keys: '0' },
  { id: 'rate.1', label: 'Rate 1 Star',  group: 'Rating', keys: '1' },
  { id: 'rate.2', label: 'Rate 2 Stars', group: 'Rating', keys: '2' },
  { id: 'rate.3', label: 'Rate 3 Stars', group: 'Rating', keys: '3' },
  { id: 'rate.4', label: 'Rate 4 Stars', group: 'Rating', keys: '4' },
  { id: 'rate.5', label: 'Rate 5 Stars', group: 'Rating', keys: '5' },

  // ── Video ──
  { id: 'video.togglePlay',   label: 'Toggle Play/Pause', group: 'Video', keys: 'Space' },
  { id: 'video.volumeUp',     label: 'Volume Up',         group: 'Video', keys: 'ArrowUp' },
  { id: 'video.volumeDown',   label: 'Volume Down',       group: 'Video', keys: 'ArrowDown' },
  { id: 'video.toggleMute',   label: 'Toggle Mute',       group: 'Video', keys: 'M' },
  { id: 'video.toggleLoop',   label: 'Toggle Loop',       group: 'Video', keys: 'L' },
  { id: 'video.rateIncrease', label: 'Speed Up',          group: 'Video', keys: ']',  keys2: 'Shift+.' },
  { id: 'video.rateDecrease', label: 'Slow Down',         group: 'Video', keys: '[',  keys2: 'Shift+,' },
  { id: 'video.rateReset',    label: 'Reset Speed',       group: 'Video', keys: 'Backspace' },
];

// ── Keyboard Presets ──
// US layout: defaults (keys is primary, keys2 is secondary/EU fallback)
// EU layout: swaps keys/keys2 for shortcuts that use AltGr-dependent characters

export type KeyboardPreset = 'us' | 'eu';

const STORAGE_KEY = 'picto-keyboard-preset';

let activePreset: KeyboardPreset = (localStorage.getItem(STORAGE_KEY) as KeyboardPreset) || 'us';

/** For EU-problematic shortcuts, swap keys and keys2 so the EU-friendly binding is primary. */
const EU_SWAP_IDS = new Set([
  'view.fitWindow',      // ` → Shift+F
  'view.toggleSidebar',  // Mod+\ → Mod+Shift+S
  'nav.back',            // Mod+[ → Mod+Alt+ArrowLeft
  'nav.forward',         // Mod+] → Mod+Alt+ArrowRight
  'video.rateIncrease',  // ] → Shift+.
  'video.rateDecrease',  // [ → Shift+,
]);

export function getKeyboardPreset(): KeyboardPreset { return activePreset; }

export function setKeyboardPreset(preset: KeyboardPreset): void {
  activePreset = preset;
  localStorage.setItem(STORAGE_KEY, preset);
  // Swap keys/keys2 in-place for EU shortcuts
  for (const def of SHORTCUT_DEFS) {
    if (!EU_SWAP_IDS.has(def.id) || !def.keys2) continue;
    if (preset === 'eu') {
      // Put EU-friendly key first
      if (!def.keys.includes('Shift+') || def.keys.includes('\\') || def.keys.includes('[') || def.keys.includes(']') || def.keys.includes('`')) {
        const tmp = def.keys;
        def.keys = def.keys2;
        def.keys2 = tmp;
      }
    } else {
      // Restore US defaults — the original SHORTCUT_DEFS array has US keys as primary
      // This is a no-op on fresh load since defaults are US
    }
  }
}

// Apply stored preset on load
if (activePreset === 'eu') setKeyboardPreset('eu');

// ── Helpers ──

export interface ShortcutGroup { name: string; items: ShortcutDef[]; }

export function getShortcutGroups(): ShortcutGroup[] {
  const map = new Map<string, ShortcutDef[]>();
  for (const def of SHORTCUT_DEFS) {
    let list = map.get(def.group);
    if (!list) { list = []; map.set(def.group, list); }
    list.push(def);
  }
  const order = ['Navigation', 'File', 'Edit', 'Rating', 'View', 'Inbox', 'Video'];
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
  return SHORTCUT_DEFS.find((d) => d.id === id);
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
