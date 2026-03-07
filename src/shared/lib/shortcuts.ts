// Centralized shortcut registry — single source of truth for all keyboard shortcuts.
// Read-only for now; persistence is a follow-up.

export interface ShortcutDef {
  id: string;
  label: string;
  description?: string;
  group: string;
  keys: string; // e.g. "Mod+A", "Escape", "Shift+Enter"
  keys2?: string; // optional secondary binding, e.g. WASD alternatives
}

const isMac =
  typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

export const SHORTCUT_DEFS: ShortcutDef[] = [
  // Navigation
  { id: 'nav.allImages',      label: 'All Images',         group: 'Navigation', keys: 'Mod+1' },
  { id: 'nav.inbox',          label: 'Inbox',              group: 'Navigation', keys: 'Mod+2' },
  { id: 'nav.untagged',       label: 'Untagged',           group: 'Navigation', keys: 'Mod+3' },
  { id: 'nav.recentViewed',   label: 'Recently Viewed',    group: 'Navigation', keys: 'Mod+4' },
  { id: 'nav.trash',          label: 'Trash',              group: 'Navigation', keys: 'Mod+5' },
  { id: 'nav.search',      label: 'Search',             group: 'Navigation', keys: 'Mod+F' },
  { id: 'nav.commandPalette', label: 'Command Palette', group: 'Navigation', keys: 'Mod+K',         description: 'Open command palette' },
  { id: 'nav.goToFolder',     label: 'Go to Folder',   group: 'Navigation', keys: 'Mod+J',         description: 'Quick-jump to a folder or smart folder' },

  // File
  { id: 'file.import',       label: 'Import Files',         group: 'File', keys: 'Mod+I' },
  { id: 'file.delete',       label: 'Delete',               group: 'File', keys: 'Mod+Backspace' },
  { id: 'file.openDefault',  label: 'Open with Default App', group: 'File', keys: 'Shift+Enter' },
  { id: 'file.newWindow',    label: 'Open in New Window',   group: 'File', keys: 'Mod+O' },
  { id: 'file.revealFolder', label: 'Reveal in Folder',     group: 'File', keys: 'Mod+Enter' },
  { id: 'file.settings',         label: 'Settings',             group: 'File', keys: 'Mod+,' },
  { id: 'file.newFolder',        label: 'New Folder',           group: 'File', keys: 'Mod+Shift+N',     description: 'Create a new folder in the sidebar' },
  { id: 'file.newSubfolder',     label: 'New Subfolder',        group: 'File', keys: 'Alt+N',           description: 'Create a subfolder under the current folder' },
  { id: 'file.newSmartFolder',   label: 'New Smart Folder',     group: 'File', keys: 'Mod+Shift+Alt+N', description: 'Create a new smart folder' },
  { id: 'file.addToFolder',      label: 'Add to Folder...',     group: 'File', keys: 'Mod+Shift+J',    description: 'Open folder picker to add selected files' },
  { id: 'file.addToLastFolder', label: 'Add to Last Folder',   group: 'File', keys: 'Shift+D',        description: 'Add selected files to the last used folder' },
  { id: 'file.removeFromFolder', label: 'Remove from Folder',   group: 'File', keys: 'Mod+Shift+Backspace', description: 'Remove selected files from current folder' },
  { id: 'file.regenerateThumbnail', label: 'Regenerate Thumbnail', group: 'File', keys: 'Mod+Shift+T', description: 'Regenerate thumbnails for selected files' },

  // Organize
  { id: 'organize.addTag',    label: 'Add Tags',        group: 'Edit', keys: 'T',     description: 'Open tag panel for selected images' },
  { id: 'organize.addFolder', label: 'Add to Folders',  group: 'Edit', keys: 'F',     description: 'Open folder picker for selected images' },

  // Edit
  { id: 'edit.undo',       label: 'Undo',           group: 'Edit', keys: 'Mod+Z' },
  { id: 'edit.redo',       label: 'Redo',           group: 'Edit', keys: 'Mod+Shift+Z' },
  { id: 'edit.selectAll',  label: 'Select All',     group: 'Edit', keys: 'Mod+A' },
  { id: 'edit.copyPath',   label: 'Copy File Path', group: 'Edit', keys: 'Mod+Alt+C' },
  { id: 'edit.copyTags',   label: 'Copy Tags',      group: 'Edit', keys: 'Mod+Shift+C' },
  { id: 'edit.pasteTags',  label: 'Paste Tags',     group: 'Edit', keys: 'Mod+Shift+V' },

  // Inbox
  { id: 'inbox.accept',  label: 'Accept',  group: 'Inbox', keys: 'Enter',     description: 'Accept inbox image (set to active)' },
  { id: 'inbox.reject',  label: 'Reject',  group: 'Inbox', keys: 'Backspace', description: 'Reject inbox image (move to trash)' },

  // View
  { id: 'view.detailView',  label: 'Detail View',     group: 'View', keys: 'Enter',     description: 'Open selected image in detail view' },
  { id: 'view.quicklook',   label: 'Quick Look',      group: 'View', keys: 'Space',     description: 'Preview selected image' },
  { id: 'view.fitWindow',   label: 'Fit to Window',   group: 'View', keys: '`' },
  { id: 'view.actualSize',  label: 'Actual Size',     group: 'View', keys: 'Mod+0' },
  { id: 'view.zoomIn',      label: 'Zoom In',         group: 'View', keys: '+' },
  { id: 'view.zoomOut',     label: 'Zoom Out',        group: 'View', keys: '-' },
  { id: 'view.grayscale',   label: 'Toggle Grayscale', group: 'View', keys: 'Mod+Alt+G', description: 'Toggle grayscale preview mode' },
  { id: 'view.slideshow',   label: 'Slideshow',        group: 'View', keys: 'F5',        description: 'Start slideshow presentation mode' },
  { id: 'view.prevImage',   label: 'Previous Image',  group: 'View', keys: 'ArrowLeft', keys2: 'A', description: 'Navigate to previous image in detail view' },
  { id: 'view.nextImage',   label: 'Next Image',      group: 'View', keys: 'ArrowRight', keys2: 'D', description: 'Navigate to next image in detail view' },
  { id: 'view.closeDetail', label: 'Close Detail',    group: 'View', keys: 'Escape',    description: 'Return to grid view' },

  // Grid navigation
  { id: 'grid.moveLeft',        label: 'Grid: Move Left',     group: 'Navigation', keys: 'ArrowLeft',  keys2: 'A', description: 'Select previous image in grid' },
  { id: 'grid.moveRight',       label: 'Grid: Move Right',    group: 'Navigation', keys: 'ArrowRight', keys2: 'D', description: 'Select next image in grid' },
  { id: 'grid.moveUp',          label: 'Grid: Move Up',       group: 'Navigation', keys: 'ArrowUp',    keys2: 'W', description: 'Select image in row above' },
  { id: 'grid.moveDown',        label: 'Grid: Move Down',     group: 'Navigation', keys: 'ArrowDown',  keys2: 'S', description: 'Select image in row below' },
  { id: 'grid.first',           label: 'First Image',         group: 'Navigation', keys: 'Home',       description: 'Select first image and scroll to top' },
  { id: 'grid.last',            label: 'Last Image',          group: 'Navigation', keys: 'End',        description: 'Select last image and scroll to bottom' },
  { id: 'grid.pageUp',          label: 'Page Up',             group: 'Navigation', keys: 'PageUp',     description: 'Jump up by one screenful' },
  { id: 'grid.pageDown',        label: 'Page Down',           group: 'Navigation', keys: 'PageDown',   description: 'Jump down by one screenful' },
  { id: 'nav.back',             label: 'Go Back',             group: 'Navigation', keys: 'Alt+ArrowLeft',  description: 'Return to previous view' },
  { id: 'nav.forward',          label: 'Go Forward',          group: 'Navigation', keys: 'Alt+ArrowRight', description: 'Go forward in view history' },
  { id: 'view.alwaysOnTop',     label: 'Always on Top',       group: 'View', keys: 'Shift+T',    description: 'Toggle window always on top' },
  { id: 'view.minimap',         label: 'Toggle Minimap',      group: 'View', keys: 'Mod+Alt+8',  description: 'Toggle navigator minimap when zoomed' },

  // Layout shortcuts
  { id: 'view.layoutGrid',      label: 'Grid Layout',         group: 'View', keys: 'Alt+1',     description: 'Switch to grid layout' },
  { id: 'view.layoutWaterfall', label: 'Waterfall Layout',    group: 'View', keys: 'Alt+2',     description: 'Switch to waterfall layout' },
  { id: 'view.layoutJustified', label: 'Justified Layout',    group: 'View', keys: 'Alt+3',     description: 'Switch to justified layout' },
  { id: 'view.toggleSidebar',   label: 'Toggle Sidebar',      group: 'View', keys: 'Mod+Alt+1', description: 'Show or hide the sidebar' },
  { id: 'view.toggleInspector', label: 'Toggle Inspector',    group: 'View', keys: 'Mod+Alt+2', description: 'Show or hide the inspector panel' },
  { id: 'view.toggleBothPanels', label: 'Toggle Panels',      group: 'View', keys: 'Tab', description: 'Show or hide sidebar and inspector together' },
  { id: 'view.toggleTileName',   label: 'Toggle Tile Name',   group: 'View', keys: 'Mod+Alt+4', description: 'Show or hide filename below tiles' },
  { id: 'view.toggleTileMetadata', label: 'Toggle Tile Info', group: 'View', keys: 'Mod+Alt+5', description: 'Show or hide resolution and extension on tiles' },

  // Edit additions
  { id: 'edit.rename',          label: 'Rename',              group: 'Edit', keys: 'Ctrl+R',    description: 'Rename selected file' },
  { id: 'edit.batchRename',     label: 'Batch Rename',        group: 'Edit', keys: 'Mod+Shift+R', description: 'Rename multiple files with a template or regex' },

  // Rating
  { id: 'rate.0', label: 'Clear Rating',  group: 'Rating', keys: '0', description: 'Remove rating from selected images' },
  { id: 'rate.1', label: 'Rate 1 Star',   group: 'Rating', keys: '1', description: 'Rate selected images 1 star' },
  { id: 'rate.2', label: 'Rate 2 Stars',  group: 'Rating', keys: '2', description: 'Rate selected images 2 stars' },
  { id: 'rate.3', label: 'Rate 3 Stars',  group: 'Rating', keys: '3', description: 'Rate selected images 3 stars' },
  { id: 'rate.4', label: 'Rate 4 Stars',  group: 'Rating', keys: '4', description: 'Rate selected images 4 stars' },
  { id: 'rate.5', label: 'Rate 5 Stars',  group: 'Rating', keys: '5', description: 'Rate selected images 5 stars' },

  // Video
  { id: 'video.togglePlay',      label: 'Toggle Play/Pause',       group: 'Video', keys: 'Space',             description: 'Play or pause video' },
  { id: 'video.volumeUp',        label: 'Volume Up',               group: 'Video', keys: 'ArrowUp',           description: 'Increase volume' },
  { id: 'video.volumeDown',      label: 'Volume Down',             group: 'Video', keys: 'ArrowDown',         description: 'Decrease volume' },
  { id: 'video.toggleMute',      label: 'Toggle Mute',             group: 'Video', keys: 'M',                 description: 'Mute or unmute video' },
  { id: 'video.toggleLoop',      label: 'Toggle Loop',             group: 'Video', keys: 'L',                 description: 'Toggle loop playback' },
  { id: 'video.rateIncrease',    label: 'Speed Up',                group: 'Video', keys: ']',                 description: 'Increase playback speed' },
  { id: 'video.rateDecrease',    label: 'Slow Down',               group: 'Video', keys: '[',                 description: 'Decrease playback speed' },
  { id: 'video.rateReset',       label: 'Reset Speed',             group: 'Video', keys: 'Backspace',         description: 'Reset playback speed to 1x' },
];

export interface ShortcutGroup {
  name: string;
  items: ShortcutDef[];
}

export function getShortcutGroups(): ShortcutGroup[] {
  const map = new Map<string, ShortcutDef[]>();
  for (const def of SHORTCUT_DEFS) {
    let list = map.get(def.group);
    if (!list) { list = []; map.set(def.group, list); }
    list.push(def);
  }
  const order = ['Navigation', 'File', 'Edit', 'Rating', 'View', 'Inbox', 'Video'];
  return order
    .filter((g) => map.has(g))
    .map((g) => ({ name: g, items: map.get(g)! }));
}

const MAC_SYMBOLS: Record<string, string> = {
  Mod: '⌘',
  Shift: '⇧',
  Alt: '⌥',
  Ctrl: '⌃',
  ArrowLeft: '←',
  ArrowRight: '→',
  ArrowUp: '↑',
  ArrowDown: '↓',
  Enter: '↩',
  Escape: 'Esc',
  Backspace: '⌫',
  Delete: '⌦',
  Space: '␣',
  Tab: '⇥',
  PageUp: 'PgUp',
  PageDown: 'PgDn',
};

const WIN_LABELS: Record<string, string> = {
  Mod: 'Ctrl',
  ArrowLeft: '←',
  ArrowRight: '→',
  ArrowUp: '↑',
  ArrowDown: '↓',
  Backspace: 'Backspace',
  Delete: 'Del',
  Escape: 'Esc',
  Enter: 'Enter',
  Space: 'Space',
  Tab: 'Tab',
  PageUp: 'PgUp',
  PageDown: 'PgDn',
};

/** Format a key string for display as a single string, e.g. "⌘⇧C" or "Ctrl+Shift+C" */
export function formatKeysDisplay(keys: string): string {
  return formatKeysAsArray(keys).join(isMac ? '' : '+');
}

/** Format a key string into individual badge tokens, e.g. ["⌘", "⇧", "C"] */
export function formatKeysAsArray(keys: string): string[] {
  // '+' is both the combo delimiter and a possible literal key.
  // Convention: literal '+' only appears as the LAST key in a combo.
  //   "+"      → ["+"]
  //   "Mod++"  → ["Mod", "+"]   (last '+' is key, second-to-last is delimiter)
  //   "Mod+0"  → ["Mod", "0"]   (normal case)
  if (keys === '+') return ['+'];

  let base = keys;
  let trailingPlus = false;
  if (keys.endsWith('++')) {
    base = keys.slice(0, -2);
    trailingPlus = true;
  }

  const parts = base.split('+').filter(Boolean);
  if (trailingPlus) parts.push('+');

  const lookup = isMac ? MAC_SYMBOLS : WIN_LABELS;
  return parts.map((p) => lookup[p] ?? p);
}

/** Look up a shortcut def by id */
export function getShortcut(id: string): ShortcutDef | undefined {
  return SHORTCUT_DEFS.find((d) => d.id === id);
}

/** Check if a KeyboardEvent matches a shortcut key string like "Mod+Shift+Z" or "ArrowRight" */
export function matchesShortcut(e: KeyboardEvent, keys: string): boolean {
  if (!keys) return false;

  // Parse the shortcut string
  let base = keys;
  let trailingPlus = false;
  if (keys === '+') {
    base = '';
    trailingPlus = true;
  } else if (keys.endsWith('++')) {
    base = keys.slice(0, -2);
    trailingPlus = true;
  }

  const parts = base ? base.split('+').filter(Boolean) : [];
  if (trailingPlus) parts.push('+');

  // Separate modifiers and key
  const modifiers = new Set<string>();
  let targetKey = '';
  for (const p of parts) {
    if (p === 'Mod' || p === 'Ctrl' || p === 'Alt' || p === 'Shift') {
      modifiers.add(p);
    } else {
      targetKey = p;
    }
  }

  // Check modifiers
  const wantMod = modifiers.has('Mod');
  const hasMod = isMac ? e.metaKey : e.ctrlKey;
  if (wantMod !== hasMod) return false;

  const wantCtrl = modifiers.has('Ctrl');
  const hasCtrl = isMac ? e.ctrlKey : false; // On Windows, Ctrl is covered by Mod
  if (wantCtrl !== hasCtrl) return false;

  if (modifiers.has('Alt') !== e.altKey) return false;
  if (modifiers.has('Shift') !== e.shiftKey) return false;

  // If no modifiers wanted, reject if any modifier is pressed (except for non-Mod cases)
  if (!wantMod && !wantCtrl && !modifiers.has('Alt') && !modifiers.has('Shift')) {
    if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return false;
  }

  // Check key
  let normalizedEventKey = e.key;
  if (e.key === ' ') normalizedEventKey = 'Space';
  else if (e.key.length === 1) normalizedEventKey = e.key.toUpperCase();

  return normalizedEventKey === targetKey;
}

/** Check if a KeyboardEvent matches a ShortcutDef (checks both primary and secondary keys) */
export function matchesShortcutDef(e: KeyboardEvent, def: ShortcutDef): boolean {
  if (matchesShortcut(e, def.keys)) return true;
  if (def.keys2 && matchesShortcut(e, def.keys2)) return true;
  return false;
}
