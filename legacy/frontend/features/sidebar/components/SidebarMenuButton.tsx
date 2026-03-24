import { useCallback, useMemo, type MouseEvent } from 'react';
import { IconMenu2 } from '@tabler/icons-react';
import { getCurrentWindow } from '#desktop/api';
import { windowController } from '../../../controllers/windowController';
import { libraryController } from '../../../controllers/libraryController';
import { useImportActionStore } from '../../../state-legacy/importActionStore';
import { useExportActionStore } from '../../../state-legacy/exportActionStore';
import { KbdTooltip } from '../../../shared/components/KbdTooltip';
import { ContextMenu, type ContextMenuEntry, useContextMenu } from '../../../shared/components/ContextMenu';
import { useNavigationStore } from '../../../state-legacy/navigationStore';
import { performRedo, performUndo } from '../../../shared/controllers/undoRedoController';
import { useUndoRedoStore } from '../../../state-legacy/undoRedoStore';
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
        { type: 'item', label: 'About Picto', onClick: () => { void windowController.openSettings(); } },
      ],
    },
    {
      type: 'submenu',
      label: 'File',
      children: [
        { type: 'item', label: 'Library Manager…', shortcut: `${modKey}+L`, onClick: () => { void windowController.openLibraryManager(); } },
        { type: 'item', label: 'Open Library…', shortcut: `${modKey}+O`, onClick: () => { void libraryController.open(); } },
        { type: 'separator' },
        { type: 'item', label: 'Import Files…', shortcut: `${modKey}+I`, onClick: () => { useImportActionStore.getState().requestImportFilesDialog(); } },
        { type: 'item', label: 'Import Folder…', shortcut: `${modKey}+Shift+I`, onClick: () => { useImportActionStore.getState().requestImportFolderDialog(); } },
        { type: 'separator' },
        { type: 'item', label: 'Export Originals…', shortcut: `${modKey}+E`, onClick: () => { useExportActionStore.getState().requestBasicExport(); } },
        { type: 'item', label: 'Export As…', shortcut: `${modKey}+Shift+E`, onClick: () => { useExportActionStore.getState().requestAdvancedExport(); } },
        { type: 'separator' },
        { type: 'item', label: 'Subscriptions…', shortcut: `${modKey}+Shift+S`, onClick: () => { void windowController.openSubscriptions(); } },
        { type: 'item', label: 'Settings…', shortcut: `${modKey}+,`, onClick: () => { void windowController.openSettings(); } },
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
        { type: 'item', label: 'All Active', shortcut: `${modKey}+1`, onClick: () => navigateTo('images') },
        { type: 'item', label: 'Inbox', shortcut: `${modKey}+2`, onClick: () => navigateTo('images', null, null, 'inbox') },
        { type: 'item', label: 'Uncategorized', onClick: () => navigateTo('images', null, null, 'uncategorized') },
        { type: 'item', label: 'Untagged', shortcut: `${modKey}+3`, onClick: () => navigateTo('images', null, null, 'untagged') },
        { type: 'item', label: 'Trash', shortcut: `${modKey}+4`, onClick: () => navigateTo('images', null, null, 'trash') },
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
