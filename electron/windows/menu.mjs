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
  openLibraryManager,
  sendToFocusedWindow,
  sendToMainWindow,
}) {
  function buildAppMenu() {
    const isMac = process.platform === 'darwin';
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
        label: 'File',
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
            click: () => sendToFocusedWindow('menu:create-library'),
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
            label: 'Import Files…',
            accelerator: 'CmdOrCtrl+I',
            click: () => sendToFocusedWindow('menu:import-files'),
          },
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
            click: () => sendToMainWindow('menu:undo'),
          },
          {
            label: 'Redo',
            accelerator: isMac ? 'Shift+Cmd+Z' : 'Ctrl+Y',
            click: () => sendToMainWindow('menu:redo'),
          },
          { type: 'separator' },
          { role: 'cut' },
          { role: 'copy' },
          { role: 'paste' },
          { role: 'selectAll' },
          ...(!isMac ? [
            { type: 'separator' },
            {
              label: 'Preferences…',
              accelerator: 'CmdOrCtrl+,',
              click: () => openSettingsWindow(),
            },
          ] : []),
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
            label: 'Recently Viewed',
            accelerator: 'CmdOrCtrl+4',
            click: () => sendToFocusedWindow('menu:navigate', 'recently_viewed'),
          },
          {
            label: 'Trash',
            accelerator: 'CmdOrCtrl+5',
            click: () => sendToFocusedWindow('menu:navigate', 'trash'),
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
