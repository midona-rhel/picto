export function createMenuManager({
  app,
  Menu,
  dialog,
  isDev,
  getCachedConfig,
  saveGlobalConfig,
  getCurrentLibraryRoot,
  libraryDisplayName,
  switchLibrary,
  openSettingsWindow,
  openSubscriptionsWindow,
  openLibraryManager,
  sendToFocusedWindow,
  sendToMainWindow,
  checkForUpdates,
  platform = process.platform,
}) {
  let shortcutBindings = {};
  let commandContext = {
    selectionCount: 0,
    singleSelected: false,
    singleKind: null,
    scopeKind: 'all',
    statusFilter: null,
    canRename: false,
    canCopyNames: false,
    canRegenerateThumbnails: false,
  };

  const accelerator = (id, fallback) => {
    if (!Object.hasOwn(shortcutBindings, id)) return fallback;
    const binding = shortcutBindings[id];
    if (typeof binding !== 'string' || binding.length === 0) return undefined;
    return binding
      .split('+')
      .filter(Boolean)
      .map((part) => part === 'Mod' ? 'CmdOrCtrl' : part)
      .join('+');
  };

  function buildAppMenu() {
    const isMac = platform === 'darwin';
    const config = getCachedConfig();
    const hasSelection = commandContext.selectionCount > 0;
    const singleMedia = commandContext.singleSelected && commandContext.singleKind === 'media';
    const inTrash = commandContext.statusFilter === 'trash';
    const inInbox = commandContext.statusFilter === 'inbox';
    const inFolder = commandContext.scopeKind === 'folder';
    const canCreateFolder = commandContext.scopeKind !== 'smart_folder';
    const canCreateSmartFolder = !['folder', 'smart_folder'].includes(commandContext.scopeKind);
    const selectionAction = (action, payload = {}) => () => {
      sendToFocusedWindow('menu:selection-action', { action, ...payload });
    };

    const pinned = config.pinnedLibraries || [];
    const history = config.libraryHistory || [];
    const recentItems = [];

    const pinnedInHistory = history.filter((libraryPath) => pinned.includes(libraryPath));
    for (const libraryPath of pinnedInHistory) {
      const name = libraryDisplayName(libraryPath);
      const isCurrent = libraryPath === getCurrentLibraryRoot();
      recentItems.push({
        label: `\u{1F4CC} ${name}`,
        type: isCurrent ? 'checkbox' : 'normal',
        checked: isCurrent,
        click: () => {
          if (libraryPath !== getCurrentLibraryRoot()) switchLibrary(libraryPath);
        },
      });
    }

    const unpinned = history.filter((libraryPath) => !pinned.includes(libraryPath));
    if (pinnedInHistory.length > 0 && unpinned.length > 0) {
      recentItems.push({ type: 'separator' });
    }

    for (const libraryPath of unpinned) {
      const name = libraryDisplayName(libraryPath);
      const isCurrent = libraryPath === getCurrentLibraryRoot();
      recentItems.push({
        label: name,
        type: isCurrent ? 'checkbox' : 'normal',
        checked: isCurrent,
        click: () => {
          if (libraryPath !== getCurrentLibraryRoot()) switchLibrary(libraryPath);
        },
      });
    }

    if (recentItems.length > 0) {
      recentItems.push({ type: 'separator' });
      recentItems.push({
        label: 'Clear History',
        click: async () => {
          const nextConfig = getCachedConfig();
          nextConfig.libraryHistory = getCurrentLibraryRoot() ? [getCurrentLibraryRoot()] : [];
          nextConfig.pinnedLibraries = nextConfig.pinnedLibraries.filter((libraryPath) => libraryPath === getCurrentLibraryRoot());
          await saveGlobalConfig(nextConfig);
          buildAppMenu();
        },
      });
    }

    const template = [
      ...(isMac
        ? [{
            label: app.name,
            submenu: [
              {
                label: 'About Picto',
                click: () => openSettingsWindow('about'),
              },
              { type: 'separator' },
              {
                label: 'Preferences…',
                accelerator: accelerator('file.settings', 'CmdOrCtrl+,'),
                click: () => openSettingsWindow(),
              },
              { type: 'separator' },
              { role: 'services' },
              { type: 'separator' },
              { role: 'hide' },
              { role: 'hideOthers' },
              { role: 'unhide' },
              { type: 'separator' },
              { role: 'quit' },
            ],
          }]
        : []),
      {
        // Libraries are a distinct concern from importing and exporting media.
        // This mirrors the way media-library apps group library switching first.
        label: 'Library',
        submenu: [
          {
            label: 'Library Manager…',
            accelerator: 'CmdOrCtrl+L',
            click: () => openLibraryManager(),
          },
          { type: 'separator' },
          {
            label: 'New Library…',
            accelerator: 'CmdOrCtrl+N',
            click: () => openLibraryManager(),
          },
          {
            label: 'Open Library…',
            click: async () => {
              const result = await dialog.showOpenDialog({
                title: 'Open Library',
                // macOS exposes package directories such as .library as files.
                properties: ['openFile', 'openDirectory'],
                message: 'Select a .library folder',
              });
              if (!result.canceled && result.filePaths.length > 0) {
                const picked = result.filePaths[0];
                if (!picked.endsWith('.library')) {
                  await dialog.showMessageBox({
                    type: 'error',
                    title: 'Invalid Library',
                    message: 'The selected folder is not a library.',
                    detail: 'Please select a folder ending in .library',
                  });
                  return;
                }
                await switchLibrary(picked);
              }
            },
          },
          { type: 'separator' },
          ...(recentItems.length > 0 ? [{ label: 'Recent Libraries', submenu: recentItems }, { type: 'separator' }] : []),
          {
            label: 'Subscriptions…',
            accelerator: 'Shift+CmdOrCtrl+S',
            click: () => sendToMainWindow('menu:navigate', 'subscriptions'),
          },
        ],
      },
      {
        label: 'File',
        submenu: [
          {
            label: inFolder ? 'New Subfolder' : 'New Folder',
            accelerator: accelerator(inFolder ? 'file.newSubfolder' : 'file.newFolder', inFolder ? 'Alt+N' : 'CmdOrCtrl+Shift+N'),
            enabled: canCreateFolder,
            click: selectionAction('new-folder'),
          },
          {
            label: 'New Smart Folder',
            accelerator: accelerator('file.newSmartFolder', 'CmdOrCtrl+Shift+Alt+N'),
            enabled: canCreateSmartFolder,
            click: selectionAction('new-smart-folder'),
          },
          { type: 'separator' },
          {
            label: 'Import Files…',
            accelerator: accelerator('file.import', 'CmdOrCtrl+I'),
            click: () => sendToMainWindow('menu:import-files'),
          },
          {
            label: 'Import Folder…',
            accelerator: 'Shift+CmdOrCtrl+I',
            click: () => sendToMainWindow('menu:import-folder'),
          },
          { type: 'separator' },
          {
            label: 'Export Originals…',
            accelerator: accelerator('file.export', 'CmdOrCtrl+E'),
            enabled: hasSelection,
            click: () => sendToMainWindow('menu:export-basic'),
          },
          {
            label: 'Export As…',
            accelerator: accelerator('file.exportAs', 'Shift+CmdOrCtrl+E'),
            enabled: hasSelection,
            click: () => sendToMainWindow('menu:export-advanced'),
          },
          ...(!isMac ? [
            { type: 'separator' },
            {
              label: 'Settings…',
              accelerator: accelerator('file.settings', 'CmdOrCtrl+,'),
              click: () => openSettingsWindow(),
            },
          ] : []),
          { type: 'separator' },
          {
            label: 'Check for Updates…',
            click: () => checkForUpdates(),
          },
          { type: 'separator' },
          isMac ? { role: 'close' } : { role: 'quit' },
        ],
      },
      {
        label: 'Edit',
        submenu: [
          {
            label: commandContext.selectionCount > 1 ? 'Batch Rename…' : 'Rename',
            accelerator: accelerator('edit.rename', 'Ctrl+R'),
            enabled: commandContext.canRename,
            click: selectionAction('rename'),
          },
          { type: 'separator' },
          {
            label: 'Undo',
            accelerator: accelerator('edit.undo', 'CmdOrCtrl+Z'),
            click: () => sendToFocusedWindow('menu:undo'),
          },
          {
            label: 'Redo',
            accelerator: accelerator('edit.redo', isMac ? 'Shift+Cmd+Z' : 'Ctrl+Y'),
            click: () => sendToFocusedWindow('menu:redo'),
          },
          { type: 'separator' },
          { role: 'cut' },
          { role: 'copy' },
          { role: 'paste' },
          {
            label: commandContext.selectionCount > 1 ? 'Copy File Paths' : 'Copy File Path',
            accelerator: accelerator('edit.copyFilePath', 'CmdOrCtrl+Alt+C'),
            enabled: hasSelection,
            click: selectionAction('copy-paths'),
          },
          {
            label: commandContext.selectionCount > 1 ? 'Copy Names' : 'Copy Name',
            enabled: commandContext.canCopyNames,
            click: selectionAction('copy-names'),
          },
          {
            label: commandContext.selectionCount > 1 ? 'Copy as Links' : 'Copy as Link',
            enabled: hasSelection,
            click: selectionAction('copy-links'),
          },
          { type: 'separator' },
          {
            label: commandContext.selectionCount > 1 ? `Regenerate ${commandContext.selectionCount} Thumbnails` : 'Regenerate Thumbnail',
            accelerator: accelerator('file.regenerateThumbnail', 'CmdOrCtrl+Shift+T'),
            enabled: commandContext.canRegenerateThumbnails,
            click: selectionAction('regenerate-thumbnails'),
          },
          { type: 'separator' },
          { role: 'selectAll' },
          {
            label: 'Deselect All',
            enabled: hasSelection,
            click: selectionAction('deselect-all'),
          },
          { type: 'separator' },
          {
            label: 'Remove from Folder',
            accelerator: accelerator('file.removeFromFolder', 'CmdOrCtrl+Shift+Backspace'),
            enabled: inFolder && hasSelection,
            click: selectionAction('remove-from-folder'),
          },
          ...(inTrash ? [
            {
              label: commandContext.selectionCount > 1 ? `Restore ${commandContext.selectionCount} Items` : 'Restore',
              accelerator: accelerator('file.restore', 'CmdOrCtrl+Shift+Backspace'),
              enabled: hasSelection,
              click: selectionAction('restore'),
            },
            {
              label: commandContext.selectionCount > 1 ? `Delete ${commandContext.selectionCount} Permanently` : 'Delete Permanently',
              accelerator: accelerator('file.delete', 'CmdOrCtrl+Backspace'),
              enabled: hasSelection,
              click: selectionAction('delete-permanently'),
            },
          ] : [{
            label: commandContext.selectionCount > 1 ? `Move ${commandContext.selectionCount} to Trash` : 'Move to Trash',
            accelerator: accelerator('file.delete', 'CmdOrCtrl+Backspace'),
            enabled: hasSelection,
            click: selectionAction('move-to-trash'),
          }]),
        ],
      },
      {
        label: 'Organize',
        submenu: [
          ...(inInbox ? [
            {
              label: commandContext.selectionCount > 1 ? `Accept ${commandContext.selectionCount} Items` : 'Accept',
              enabled: hasSelection,
              click: selectionAction('accept'),
            },
            {
              label: commandContext.selectionCount > 1 ? `Reject ${commandContext.selectionCount} Items` : 'Reject',
              enabled: hasSelection,
              click: selectionAction('reject'),
            },
            { type: 'separator' },
          ] : []),
          {
            label: 'Add Tags…',
            enabled: hasSelection,
            click: selectionAction('add-tags'),
          },
          {
            label: 'Add to Folder…',
            accelerator: accelerator('file.addToFolder', 'CmdOrCtrl+Shift+J'),
            enabled: hasSelection,
            click: selectionAction('add-to-folder'),
          },
          {
            label: 'Auto Tag…',
            accelerator: accelerator('organize.autoTag', 'CmdOrCtrl+Shift+A'),
            enabled: hasSelection,
            click: selectionAction('auto-tag'),
          },
          { type: 'separator' },
          {
            label: 'Set Rating',
            enabled: hasSelection,
            submenu: [0, 1, 2, 3, 4, 5].map((rating) => ({
              label: rating === 0 ? 'No Rating' : '★'.repeat(rating),
              enabled: hasSelection,
              click: selectionAction('set-rating', { rating }),
            })),
          },
          { type: 'separator' },
          {
            label: 'Tags',
            click: () => sendToFocusedWindow('menu:navigate', 'tags'),
          },
        ],
      },
      {
        label: 'View',
        submenu: [
          {
            label: 'Open with Default App',
            accelerator: accelerator('file.openDefaultApp', 'Shift+Enter'),
            enabled: singleMedia,
            click: selectionAction('open-default'),
          },
          {
            label: platform === 'darwin' ? 'Reveal in Finder' : 'Show in Explorer',
            accelerator: accelerator('file.revealInFolder', 'CmdOrCtrl+Enter'),
            enabled: singleMedia,
            click: selectionAction('reveal-in-folder'),
          },
          {
            label: 'Open in New Window',
            accelerator: accelerator('file.openNewWindow', 'CmdOrCtrl+O'),
            enabled: commandContext.singleSelected,
            click: selectionAction('open-new-window'),
          },
          { type: 'separator' },
          {
            label: 'All Images',
            accelerator: accelerator('nav.allActive', 'CmdOrCtrl+1'),
            click: () => sendToFocusedWindow('menu:navigate', 'images'),
          },
          {
            label: 'Inbox',
            accelerator: accelerator('nav.inbox', 'CmdOrCtrl+2'),
            click: () => sendToFocusedWindow('menu:navigate', 'review'),
          },
          {
            label: 'Untagged',
            accelerator: accelerator('nav.untagged', 'CmdOrCtrl+3'),
            click: () => sendToFocusedWindow('menu:navigate', 'untagged'),
          },
          {
            label: 'Trash',
            accelerator: accelerator('nav.trash', 'CmdOrCtrl+4'),
            click: () => sendToFocusedWindow('menu:navigate', 'trash'),
          },
          { type: 'separator' },
          {
            label: 'Duplicates',
            click: () => sendToFocusedWindow('menu:navigate', 'duplicates'),
          },
          { type: 'separator' },
          {
            label: 'Log Window',
            accelerator: accelerator('view.toggleLogs', 'CmdOrCtrl+L'),
            click: () => sendToMainWindow('menu:toggle-diagnostics'),
          },
          { type: 'separator' },
          ...(isDev ? [
            { role: 'reload' },
            { role: 'forceReload' },
            { role: 'toggleDevTools' },
            { type: 'separator' },
          ] : []),
          { role: 'togglefullscreen' },
        ],
      },
      {
        label: 'Window',
        submenu: [
          { role: 'minimize' },
          { role: 'zoom' },
          ...(isMac ? [{ type: 'separator' }, { role: 'front' }] : [{ role: 'close' }]),
        ],
      },
      {
        label: 'Help',
        submenu: [
          {
            label: 'About Picto',
            click: () => openSettingsWindow('about'),
          },
        ],
      },
    ];

    const menu = Menu.buildFromTemplate(template);
    Menu.setApplicationMenu(menu);
  }

  function setShortcutBindings(bindings) {
    shortcutBindings = bindings && typeof bindings === 'object' && !Array.isArray(bindings)
      ? { ...bindings }
      : {};
    buildAppMenu();
  }

  function setCommandContext(context) {
    commandContext = {
      ...commandContext,
      ...(context && typeof context === 'object' && !Array.isArray(context) ? context : {}),
    };
    buildAppMenu();
  }

  return { buildAppMenu, setShortcutBindings, setCommandContext };
}
