import { useCallback, useMemo, type MouseEvent } from 'react';
import { IconMenu2 } from '@tabler/icons-react';
import { api, getCurrentWindow, invoke, libraryHost } from '#desktop/api';
import { KbdTooltip } from '../ui/KbdTooltip';
import { ContextMenu, type ContextMenuEntry, useContextMenu } from '../ui/ContextMenu';
import { useNavigationStore } from '../../stores/navigationStore';
import { performRedo, performUndo } from '../../controllers/undoRedoController';
import { useUndoRedoStore } from '../../stores/undoRedoStore';
import styles from './SidebarMenuButton.module.css';

export function SidebarMenuButton() {
  const contextMenu = useContextMenu();
  const navigateTo = useNavigationStore((s) => s.navigateTo);
  const undoCount = useUndoRedoStore((s) => s.undoStack.length);
  const redoCount = useUndoRedoStore((s) => s.redoStack.length);
  const undoBusy = useUndoRedoStore((s) => s.inFlight);
  const win = getCurrentWindow();

  const platform = navigator.platform ?? '';
  const isMac = platform.includes('Mac');
  const isLinux = platform.includes('Linux');
  const isWindows = platform.includes('Win');
  const modKey = isMac ? 'Cmd' : 'Ctrl';

  // Visibility rule:
  // - Windows/Linux: always visible
  // - macOS: dev-only
  if (!(isWindows || isLinux || (isMac && import.meta.env.DEV))) return null;

  const menuItems = useMemo<ContextMenuEntry[]>(() => ([
    {
      type: 'submenu',
      label: 'Picto',
      children: [
        { type: 'item', label: 'Duplicates', onClick: () => navigateTo('duplicates') },
        { type: 'item', label: 'Tag Manager', onClick: () => navigateTo('tags') },
        { type: 'separator' },
        { type: 'item', label: 'About Picto', onClick: () => { void api.os.openSettingsWindow(); } },
      ],
    },
    {
      type: 'submenu',
      label: 'File',
      children: [
        { type: 'item', label: 'Library Manager…', shortcut: `${modKey}+L`, onClick: () => { void invoke('open_library_manager'); } },
        { type: 'item', label: 'Open Library…', shortcut: `${modKey}+O`, onClick: () => { void libraryHost.open(); } },
        { type: 'separator' },
        { type: 'item', label: 'Subscriptions…', shortcut: `${modKey}+Shift+S`, onClick: () => { void api.os.openSubscriptionsWindow(); } },
        { type: 'item', label: 'Settings…', shortcut: `${modKey}+,`, onClick: () => { void api.os.openSettingsWindow(); } },
      ],
    },
    {
      type: 'submenu',
      label: 'Edit',
      children: [
        {
          type: 'item',
          label: 'Undo',
          shortcut: `${modKey}+Z`,
          disabled: undoBusy || undoCount === 0,
          onClick: () => { void performUndo(); },
        },
        {
          type: 'item',
          label: 'Redo',
          shortcut: isMac ? 'Cmd+Shift+Z' : 'Ctrl+Y',
          disabled: undoBusy || redoCount === 0,
          onClick: () => { void performRedo(); },
        },
      ],
    },
    {
      type: 'submenu',
      label: 'View',
      children: [
        { type: 'item', label: 'All Images', shortcut: `${modKey}+1`, onClick: () => navigateTo('images') },
        { type: 'item', label: 'Inbox', shortcut: `${modKey}+2`, onClick: () => navigateTo('images', null, null, 'inbox') },
        { type: 'item', label: 'Uncategorized', onClick: () => navigateTo('images', null, null, 'uncategorized') },
        { type: 'item', label: 'Untagged', shortcut: `${modKey}+3`, onClick: () => navigateTo('images', null, null, 'untagged') },
        { type: 'item', label: 'Recently Viewed', shortcut: `${modKey}+4`, onClick: () => navigateTo('images', null, null, 'recently_viewed') },
        { type: 'item', label: 'Trash', shortcut: `${modKey}+5`, onClick: () => navigateTo('images', null, null, 'trash') },
      ],
    },
    {
      type: 'submenu',
      label: 'Window',
      children: [
        { type: 'item', label: 'Minimize', onClick: () => { void win.minimize(); } },
        { type: 'item', label: 'Zoom', onClick: () => { void win.toggleMaximize(); } },
        { type: 'item', label: 'Close', onClick: () => { void win.close(); } },
      ],
    },
  ]), [
    navigateTo,
    undoBusy,
    undoCount,
    redoCount,
    modKey,
    isMac,
    win,
  ]);

  const handleClick = useCallback((e: MouseEvent<HTMLButtonElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    contextMenu.openAt(
      { x: Math.round(rect.left), y: Math.round(rect.bottom + 6) },
      menuItems,
    );
  }, [contextMenu, menuItems]);

  return (
    <>
      <KbdTooltip label="Menu">
        <button className={styles.button} onClick={handleClick}>
          <IconMenu2 size={16} />
        </button>
      </KbdTooltip>
      {contextMenu.state && (
        <ContextMenu
          items={contextMenu.state.items}
          position={contextMenu.state.position}
          onClose={contextMenu.close}
          searchable={false}
          iconGutter={false}
          panelWidth={130}
        />
      )}
    </>
  );
}
