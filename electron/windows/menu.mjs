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
  platform = process.platform,
}) {
  function buildAppMenu() {
    const isMac = platform === 'darwin';
    const config = getCachedConfig();

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
              { role: 'about' },
              { type: 'separator' },
              {
                label: 'Preferences…',
                accelerator: 'CmdOrCtrl+,',
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
            accelerator: 'CmdOrCtrl+O',
            click: async () => {
              const result = await dialog.showOpenDialog({
                title: 'Open Library',
                properties: ['openDirectory'],
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
            label: 'Import Files…',
            accelerator: 'CmdOrCtrl+I',
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
            accelerator: 'CmdOrCtrl+E',
            click: () => sendToMainWindow('menu:export-basic'),
          },
          {
            label: 'Export As…',
            accelerator: 'Shift+CmdOrCtrl+E',
            click: () => sendToMainWindow('menu:export-advanced'),
          },
          ...(!isMac ? [
            { type: 'separator' },
            {
              label: 'Settings…',
              accelerator: 'CmdOrCtrl+,',
              click: () => openSettingsWindow(),
            },
          ] : []),
          { type: 'separator' },
          isMac ? { role: 'close' } : { role: 'quit' },
        ],
      },
      {
        label: 'Edit',
        submenu: [
          {
            label: 'Undo',
            accelerator: 'CmdOrCtrl+Z',
            click: () => sendToFocusedWindow('menu:undo'),
          },
          {
            label: 'Redo',
            accelerator: isMac ? 'Shift+Cmd+Z' : 'Ctrl+Y',
            click: () => sendToFocusedWindow('menu:redo'),
          },
          { type: 'separator' },
          { role: 'cut' },
          { role: 'copy' },
          { role: 'paste' },
          { role: 'selectAll' },
        ],
      },
      {
        label: 'Organize',
        submenu: [
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
            label: 'All Images',
            accelerator: 'CmdOrCtrl+1',
            click: () => sendToFocusedWindow('menu:navigate', 'images'),
          },
          {
            label: 'Inbox',
            accelerator: 'CmdOrCtrl+2',
            click: () => sendToFocusedWindow('menu:navigate', 'review'),
          },
          {
            label: 'Untagged',
            accelerator: 'CmdOrCtrl+3',
            click: () => sendToFocusedWindow('menu:navigate', 'untagged'),
          },
          {
            label: 'Trash',
            accelerator: 'CmdOrCtrl+4',
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
            accelerator: 'CmdOrCtrl+Shift+L',
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
            click: () => openSettingsWindow(),
          },
        ],
      },
    ];

    const menu = Menu.buildFromTemplate(template);
    Menu.setApplicationMenu(menu);
  }

  return { buildAppMenu };
}
