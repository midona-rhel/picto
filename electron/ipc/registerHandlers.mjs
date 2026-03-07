import { shell } from 'electron';

function createReverseSearchConfigs() {
  const waitForHelper = `
    function __waitFor(sel, timeout = 10000) {
      return new Promise((resolve, reject) => {
        if (document.querySelector(sel)) return resolve(document.querySelector(sel));
        const obs = new MutationObserver(() => {
          const el = document.querySelector(sel);
          if (el) { obs.disconnect(); resolve(el); }
        });
        obs.observe(document.documentElement, { childList: true, subtree: true, attributes: true });
        setTimeout(() => { obs.disconnect(); reject(new Error('Timeout waiting for: ' + sel)); }, timeout);
      });
    }
  `;

  return {
    tineye: {
      url: 'https://tineye.com/',
      preSetup: `(async () => { ${waitForHelper} await __waitFor('#upload-box'); })()`,
      fileInputSelector: "input[type='file']",
      postSetup: null,
      isResultUrl: (href) => /tineye\.com\/search\//.test(href),
    },
    saucenao: {
      url: 'https://saucenao.com/',
      preSetup: `(async () => { ${waitForHelper} await __waitFor('#searchForm'); })()`,
      fileInputSelector: "input[type='file']",
      postSetup: `document.querySelector('#searchForm')?.submit()`,
      isResultUrl: (href) => href.includes('saucenao.com/search.php'),
    },
    yandex: {
      url: 'https://yandex.com/images/',
      preSetup: `(async () => {
        ${waitForHelper}
        await __waitFor('.input__cbir-button, [data-bem*="cbir"], .HeaderDesktopForm-CbirButton, button[aria-label*="image"]');
        const btn = document.querySelector('.input__cbir-button')
          || document.querySelector('[data-bem*="cbir"]')
          || document.querySelector('.HeaderDesktopForm-CbirButton')
          || document.querySelector('button[aria-label*="image"]');
        if (btn) { btn.click(); await new Promise(r => setTimeout(r, 1000)); }
        await __waitFor("input[type='file']", 8000);
      })()`,
      fileInputSelector: "input[type='file']",
      postSetup: null,
      isResultUrl: (href) => /yandex\.(com|eu|ru)\/images\/search/.test(href),
    },
    sogou: {
      url: 'https://pic.sogou.com/',
      preSetup: `(async () => {
        ${waitForHelper}
        await __waitFor('#cameraIco, .camera-ico, [class*="camera"]');
        const btn = document.querySelector('#cameraIco') || document.querySelector('.camera-ico') || document.querySelector('[class*="camera"]');
        if (btn) { btn.click(); await new Promise(r => setTimeout(r, 800)); }
        await __waitFor("input[type='file']", 5000);
      })()`,
      fileInputSelector: "input[type='file']",
      postSetup: null,
      isResultUrl: (href) => href.includes('/ris'),
    },
    bing: {
      url: 'https://www.bing.com/images',
      preSetup: `(async () => {
        ${waitForHelper}
        await __waitFor('#sb_sbip, #sb_sbi, [id*="sbi"], input[type="file"]');
        const btn = document.querySelector('#sb_sbip') || document.querySelector('#sb_sbi') || document.querySelector('[id*="sbi"]');
        if (btn) { btn.click(); await new Promise(r => setTimeout(r, 1000)); }
        await __waitFor("input[type='file']", 8000);
      })()`,
      fileInputSelector: "input[type='file']",
      postSetup: null,
      isResultUrl: (href) => href.includes('bing.com/images/search'),
    },
  };
}

async function runReverseImageSearch({ BrowserWindow, filePath, engine }) {
  const configs = createReverseSearchConfigs();
  const cfg = configs[engine];
  if (!cfg) throw new Error(`Unknown search engine: ${engine}`);

  const searchWin = new BrowserWindow({
    show: false,
    width: 1100,
    height: 800,
    webPreferences: { contextIsolation: true, nodeIntegration: false },
  });
  searchWin.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));

  try {
    console.log(`[reverse-search] ${engine}: loading ${cfg.url}`);
    await Promise.race([
      searchWin.webContents.loadURL(cfg.url),
      new Promise((_, rej) => setTimeout(() => rej(new Error('Page load timed out')), 15000)),
    ]);

    console.log(`[reverse-search] ${engine}: running pre-setup`);
    await Promise.race([
      searchWin.webContents.executeJavaScript(cfg.preSetup, true),
      new Promise((_, rej) => setTimeout(() => rej(new Error('Pre-setup timed out')), 15000)),
    ]);

    console.log(`[reverse-search] ${engine}: injecting file via CDP`);
    searchWin.webContents.debugger.attach('1.3');
    try {
      const { root } = await searchWin.webContents.debugger.sendCommand('DOM.getDocument');
      const { nodeId } = await searchWin.webContents.debugger.sendCommand('DOM.querySelector', {
        nodeId: root.nodeId,
        selector: cfg.fileInputSelector,
      });
      if (!nodeId) throw new Error(`File input not found: ${cfg.fileInputSelector}`);
      await searchWin.webContents.debugger.sendCommand('DOM.setFileInputFiles', {
        nodeId,
        files: [filePath],
      });
    } finally {
      try {
        searchWin.webContents.debugger.detach();
      } catch {}
    }

    if (cfg.postSetup) {
      await searchWin.webContents.executeJavaScript(cfg.postSetup, true);
    }

    const resultUrl = await new Promise((resolve, reject) => {
      let attempts = 0;
      const interval = setInterval(async () => {
        attempts++;
        if (searchWin.isDestroyed()) {
          clearInterval(interval);
          reject(new Error('Search window was closed'));
          return;
        }
        try {
          const href = await searchWin.webContents.executeJavaScript('location.href', true);
          if (cfg.isResultUrl(href)) {
            clearInterval(interval);
            resolve(href);
          }
          if (attempts > 300) {
            clearInterval(interval);
            reject(new Error(`${engine}: timed out. Last URL: ${href}`));
          }
        } catch {}
      }, 100);
    });

    await shell.openExternal(resultUrl);
    return resultUrl;
  } finally {
    if (!searchWin.isDestroyed()) searchWin.destroy();
  }
}

export function registerIpcHandlers({
  ipcMain,
  BrowserWindow,
  Menu,
  nativeImage,
  clipboard,
  dialog,
  nativeTheme,
  screen,
  invoke,
  isValidHash,
  buildBlobPath,
  windowManager,
  libraryService,
}) {
  ipcMain.handle('picto:invoke', async (_event, payload) => {
    const { command, args } = payload || {};
    if (!command || typeof command !== 'string') {
      throw new Error('Invalid invoke payload');
    }

    if (command === 'open_settings_window') {
      windowManager.openSettingsWindow();
      return null;
    }
    if (command === 'open_subscriptions_window') {
      windowManager.openSubscriptionsWindow();
      return null;
    }
    if (command === 'open_library_manager') {
      windowManager.openLibraryManager();
      return null;
    }
    if (command === 'open_in_new_window') {
      const hash = args?.hash;
      if (!isValidHash(hash)) throw new Error('Invalid hash for detail window');
      const label = `detail-${hash.slice(0, 12)}`;
      const existing = windowManager.getWindow(label);
      if (existing && !existing.isDestroyed()) {
        existing.focus();
        return null;
      }
      const { width, height } = windowManager.calcDetailWindowSize(args?.width, args?.height);
      windowManager.createWindow(label, hash, width, height);
      return null;
    }

    return invoke(command, args || {});
  });

  ipcMain.handle('picto:event:emit', (event, { name, payload, target }) => {
    if (!name || typeof name !== 'string') return null;
    if (target) {
      const win = windowManager.getWindow(target);
      if (win && !win.isDestroyed()) win.webContents.send(name, payload);
      return null;
    }
    for (const win of BrowserWindow.getAllWindows()) {
      if (win.webContents.id !== event.sender.id) {
        win.webContents.send(name, payload);
      }
    }
    return null;
  });

  ipcMain.handle('picto:window', (event, { method, payload }) => {
    const win = BrowserWindow.fromWebContents(event.sender);
    if (!win) throw new Error('No window context');
    switch (method) {
      case 'show': win.show(); return null;
      case 'setTheme': {
        const theme = payload?.theme;
        if (theme === 'dark' || theme === 'light' || theme === 'system') {
          nativeTheme.themeSource = theme;
        }
        return null;
      }
      case 'startDragging':
        // Electron window dragging is driven by CSS app regions. This call is
        // retained only for frontend compatibility with older host abstractions.
        return null;
      case 'minimize': win.minimize(); return null;
      case 'toggleMaximize': win.isMaximized() ? win.unmaximize() : win.maximize(); return null;
      case 'setSize': {
        const width = Number(payload?.width);
        const height = Number(payload?.height);
        if (!Number.isFinite(width) || !Number.isFinite(height)) throw new Error('Invalid size');
        win.setSize(Math.round(width), Math.round(height));
        return null;
      }
      case 'setAlwaysOnTop':
        win.setAlwaysOnTop(Boolean(payload?.value));
        return null;
      case 'close':
        win.close();
        return null;
      case 'setFocus':
        win.focus();
        return null;
      default:
        throw new Error(`Unknown window method: ${method}`);
    }
  });

  ipcMain.handle('picto:dialog:open', async (_event, options = {}) => {
    const result = await dialog.showOpenDialog({
      properties: ['openFile', 'multiSelections'],
      ...options,
    });
    if (result.canceled) return null;
    if (result.filePaths.length === 0) return null;
    return result.filePaths.length === 1 ? result.filePaths[0] : result.filePaths;
  });

  ipcMain.handle('picto:dialog:save', async (_event, options = {}) => {
    const result = await dialog.showSaveDialog(options);
    return result.canceled ? null : result.filePath ?? null;
  });

  ipcMain.handle('picto:clipboard:writeText', (_event, { text }) => {
    clipboard.writeText(String(text ?? ''));
    return null;
  });

  ipcMain.handle('picto:clipboard:copyFile', async (_event, { filePath }) => {
    if (process.platform === 'darwin') {
      clipboard.writeBookmark(filePath.split('/').pop(), `file://${filePath}`);
    } else {
      clipboard.writeText(filePath);
    }
    return null;
  });

  ipcMain.handle('picto:clipboard:copyImage', async (_event, { filePath }) => {
    try {
      const img = nativeImage.createFromPath(filePath);
      if (img.isEmpty()) throw new Error('Failed to load image');
      clipboard.writeImage(img);
      return null;
    } catch (err) {
      throw new Error(`Failed to copy image: ${err.message}`);
    }
  });

  ipcMain.handle('picto:reverseImageSearch', async (_event, { filePath, engine }) => {
    return runReverseImageSearch({ BrowserWindow, filePath, engine });
  });

  ipcMain.handle('picto:drag:start', async (event, { hashes, iconDataUrl }) => {
    if (!hashes?.length) return null;
    let filePath;
    try {
      filePath = await invoke('resolve_file_path', { hash: hashes[0] });
    } catch {
      return null;
    }
    if (!filePath) return null;

    let icon;
    if (iconDataUrl) {
      try {
        icon = nativeImage.createFromDataURL(iconDataUrl);
        if (icon.isEmpty()) icon = null;
      } catch {
        icon = null;
      }
    }
    if (!icon) {
      const thumbPath = buildBlobPath('thumb', hashes[0], 'jpg');
      try {
        icon = nativeImage.createFromPath(thumbPath);
        if (icon.isEmpty()) icon = null;
      } catch {
        icon = null;
      }
      if (icon) icon = icon.resize({ width: 64 });
    }

    event.sender.startDrag({
      files: [filePath],
      icon: icon || nativeImage.createEmpty(),
    });

    return { ok: true };
  });

  ipcMain.handle('picto:popup-menu', (event) => {
    const win = BrowserWindow.fromWebContents(event.sender);
    if (win) Menu.getApplicationMenu()?.popup({ window: win });
  });

  ipcMain.handle('picto:monitor:current', () => {
    const point = screen.getCursorScreenPoint();
    const display = screen.getDisplayNearestPoint(point);
    return {
      scaleFactor: display.scaleFactor,
      size: { width: display.size.width, height: display.size.height },
    };
  });

  ipcMain.handle('picto:library:create', async (_event, payload) => libraryService.createLibrary(payload));
  ipcMain.handle('picto:library:open', async () => libraryService.openLibraryDialog());
  ipcMain.handle('picto:library:switch', async (_event, { path }) => libraryService.switchLibrary(path));
  ipcMain.handle('picto:library:remove', async (_event, { path }) => libraryService.removeLibrary(path));
  ipcMain.handle('picto:library:delete', async (_event, { path }) => libraryService.deleteLibrary(path));
  ipcMain.handle('picto:library:togglePin', async (_event, { path }) => libraryService.toggleLibraryPin(path));
  ipcMain.handle('picto:library:rename', async (_event, { path, newName }) => libraryService.renameLibrary(path, newName));
  ipcMain.handle('picto:library:relocate', async (_event, { oldPath }) => libraryService.relocateLibrary(oldPath));
  ipcMain.handle('picto:library:getConfig', async () => libraryService.getLibraryConfig());
}
