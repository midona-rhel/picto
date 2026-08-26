import { shell } from 'electron';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { clipboardFilePaths, clipboardHasImport } from './clipboardImport.mjs';
import { createTrustedIpcHandle } from './trustedIpc.mjs';

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
      fileInputSelector: '#fileInput',
      postSetup: `(async () => {
        const input = document.querySelector('#fileInput');
        if (!input?.files?.length) throw new Error('SauceNAO did not receive the image');
        checkImageFile(input);
        const started = Date.now();
        while (!searchReady && Date.now() - started < 5000) {
          await new Promise(resolve => setTimeout(resolve, 25));
        }
        if (!searchReady) throw new Error('SauceNAO rejected the image');
        document.querySelector('#searchForm')?.requestSubmit();
      })()`,
      isResultUrl: (href) => href.includes('saucenao.com/search.php'),
      keepResultWindow: true,
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
        const btn = await __waitFor('#sb_sbi, #sb_sbip');
        btn.click();
        await __waitFor('#sb_fileinput', 8000);
      })()`,
      fileInputSelector: '#sb_fileinput',
      postSetup: `(() => {
        const input = document.querySelector('#sb_fileinput');
        if (!input?.files?.length) throw new Error('Bing did not receive the image');
        input.dispatchEvent(new Event('input', { bubbles: true }));
        input.dispatchEvent(new Event('change', { bubbles: true }));
      })()`,
      isResultUrl: (href) => /bing\.com\/images\/(search|searchbyimage)/.test(href),
    },
  };
}

function waitForReverseSearchResult(searchWin, cfg, engine, timeoutMs = 30_000) {
  return new Promise((resolve, reject) => {
    let interval;
    let timeout;
    const cleanup = () => {
      clearInterval(interval);
      clearTimeout(timeout);
      searchWin.webContents.removeListener('did-navigate', checkUrl);
      searchWin.webContents.removeListener('did-navigate-in-page', checkUrl);
      searchWin.removeListener('closed', onClosed);
    };
    const finish = (callback, value) => {
      cleanup();
      callback(value);
    };
    const checkUrl = (_event, navigatedUrl) => {
      const href = typeof navigatedUrl === 'string'
        ? navigatedUrl
        : searchWin.webContents.getURL();
      if (cfg.isResultUrl(href)) finish(resolve, href);
    };
    const onClosed = () => finish(reject, new Error('Search window was closed'));

    searchWin.webContents.on('did-navigate', checkUrl);
    searchWin.webContents.on('did-navigate-in-page', checkUrl);
    searchWin.on('closed', onClosed);
    interval = setInterval(checkUrl, 100);
    timeout = setTimeout(() => {
      const href = searchWin.isDestroyed() ? '(closed)' : searchWin.webContents.getURL();
      finish(reject, new Error(`${engine}: timed out. Last URL: ${href}`));
    }, timeoutMs);
    checkUrl();
  });
}

export async function runReverseImageSearch({
  BrowserWindow,
  filePath,
  engine,
  openExternal = (url) => shell.openExternal(url),
}) {
  const configs = createReverseSearchConfigs();
  const cfg = configs[engine];
  if (!cfg) throw new Error(`Unknown search engine: ${engine}`);

  const searchWin = new BrowserWindow({
    show: false,
    width: 1100,
    height: 800,
    webPreferences: { contextIsolation: true, nodeIntegration: false, sandbox: true },
  });
  searchWin.webContents.setWindowOpenHandler(({ url }) => {
    void openExternal(url);
    return { action: 'deny' };
  });
  let keepResultWindow = false;

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

    const resultUrl = await waitForReverseSearchResult(searchWin, cfg, engine);

    if (cfg.keepResultWindow) {
      keepResultWindow = true;
      searchWin.show();
      return resultUrl;
    }

    await openExternal(resultUrl);
    return resultUrl;
  } finally {
    if (!keepResultWindow && !searchWin.isDestroyed()) searchWin.destroy();
  }
}

export function registerIpcHandlers({
  app,
  ipcMain,
  BrowserWindow,
  Menu,
  nativeImage,
  clipboard,
  dialog,
  nativeTheme,
  screen,
  invoke,
  invokeSerialized,
  isValidHash,
  buildBlobPath,
  setThumbnail,
  regenerateThumbnail,
  windowManager,
  libraryService,
  updaterService,
  siteIconService,
  startNativeDrag,
  getAssociatedApplications,
  openWithApplication,
  isDev,
}) {
  const openWithOptionsByExtension = new Map();
  const handle = createTrustedIpcHandle(ipcMain, windowManager.ownsWebContents);

  handle('picto:invoke', async (_event, payload) => {
    const { command, args } = payload || {};
    if (!command || typeof command !== 'string') {
      throw new Error('Invalid invoke payload');
    }

    if (command === 'open_settings_window') {
      windowManager.openSettingsWindow();
      return null;
    }
    if (command === 'auth_session_start') {
      return windowManager.startAuthSession(args?.site_category);
    }
    if (command === 'auth_session_cancel') {
      return windowManager.cancelAuthSession();
    }
    if (command === 'auth_session_state') {
      return windowManager.getAuthSessionState();
    }
    if (command === 'auth_onlyfans_manual') {
      return windowManager.saveManualOnlyFansCredential(args);
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
      const itemId = Number(args?.item_id);
      const hasHash = isValidHash(hash);
      const hasItemId = Number.isSafeInteger(itemId) && itemId > 0;
      if (hasHash === hasItemId) throw new Error('Detail window requires exactly one media hash or group item ID');
      const label = hasHash ? `detail-${hash.slice(0, 12)}` : `detail-group-${itemId}`;
      const existing = windowManager.getWindow(label);
      if (existing && !existing.isDestroyed()) {
        existing.focus();
        return null;
      }
      const { width, height } = windowManager.calcDetailWindowSize(args?.width, args?.height);
      windowManager.createWindow(label, hasHash ? hash : null, width, height, hasItemId ? itemId : null);
      return null;
    }
    if (command === 'media.set_thumbnail') {
      await setThumbnail(args?.file_hash, args?.png_base64);
      for (const win of BrowserWindow.getAllWindows()) {
        if (!win.isDestroyed()) win.webContents.send('picto:thumbnail-changed', { fileHash: args?.file_hash });
      }
      return null;
    }
    if (command === 'media.regenerate_thumbnails') {
      const hashes = [...new Set(args?.file_hashes ?? [])];
      if (hashes.length === 0 || hashes.some((hash) => !isValidHash(hash))) {
        throw new Error('Invalid thumbnail targets');
      }
      for (const hash of hashes) {
        await regenerateThumbnail(hash);
        for (const win of BrowserWindow.getAllWindows()) {
          if (!win.isDestroyed()) win.webContents.send('picto:thumbnail-changed', { fileHash: hash });
        }
      }
      return {
        requested: hashes.length,
        enqueued: hashes.length,
        already_queued: 0,
        receipt: { revision: 0, resources: ['thumbnails'], item_ids: [] },
      };
    }
    const nativeStarted = performance.now();
    const serialized = await invokeSerialized(command, args || {});
    return {
      __pictoCoreJson: serialized,
      __pictoNativeMs: performance.now() - nativeStarted,
    };
  });

  // Restart the main window (e.g. after changing to a native transparency theme)
  handle('picto:restart-main-window', () => {
    const main = windowManager.getMainWindow();
    if (main && !main.isDestroyed()) {
      main.close();
    }
    // Small delay so the old window fully closes before creating a new one
    setTimeout(() => { windowManager.createWindow('main'); }, 200);
  });

  handle('picto:event:emit', (event, { name, payload, target }) => {
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

  handle('picto:window', async (event, { method, payload }) => {
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
      case 'captureRect': {
        const bounds = win.getContentBounds();
        const x = Math.max(0, Math.round(Number(payload?.x)));
        const y = Math.max(0, Math.round(Number(payload?.y)));
        const width = Math.min(bounds.width - x, Math.round(Number(payload?.width)));
        const height = Math.min(bounds.height - y, Math.round(Number(payload?.height)));
        if (!(width > 0) || !(height > 0)) throw new Error('Invalid capture bounds');
        return (await win.webContents.capturePage({ x, y, width, height })).toDataURL();
      }
      default:
        throw new Error(`Unknown window method: ${method}`);
    }
  });

  handle('picto:dialog:open', async (event, options = {}) => {
    const win = BrowserWindow.fromWebContents(event.sender) ?? undefined;
    const {
      properties: requestedProperties,
      multiple,
      ...rest
    } = options ?? {};

    const properties = new Set(
      Array.isArray(requestedProperties) && requestedProperties.length > 0
        ? requestedProperties
        : ['openFile'],
    );
    if (multiple !== false) {
      properties.add('multiSelections');
    } else {
      properties.delete('multiSelections');
    }
    if (!properties.has('openFile') && !properties.has('openDirectory')) {
      properties.add('openFile');
    }

    const result = await dialog.showOpenDialog(win, {
      ...rest,
      properties: [...properties],
    });
    if (result.canceled) return null;
    if (result.filePaths.length === 0) return null;
    return result.filePaths.length === 1 ? result.filePaths[0] : result.filePaths;
  });

  handle('picto:dialog:save', async (_event, options = {}) => {
    const result = await dialog.showSaveDialog(options);
    return result.canceled ? null : result.filePath ?? null;
  });

  handle('picto:clipboard:writeText', (_event, { text }) => {
    clipboard.writeText(String(text ?? ''));
    return null;
  });

  handle('picto:clipboard:copyFile', async (_event, { filePath }) => {
    if (process.platform === 'darwin') {
      clipboard.writeBookmark(filePath.split('/').pop(), `file://${filePath}`);
    } else {
      clipboard.writeText(filePath);
    }
    return null;
  });

  handle('picto:clipboard:copyImage', async (_event, { filePath }) => {
    try {
      const img = nativeImage.createFromPath(filePath);
      if (img.isEmpty()) throw new Error('Failed to load image');
      clipboard.writeImage(img);
      return null;
    } catch (err) {
      throw new Error(`Failed to copy image: ${err.message}`);
    }
  });

  handle('picto:clipboard:hasImport', () => clipboardHasImport(clipboard));

  handle('picto:clipboard:readImport', () => {
    const paths = clipboardFilePaths(clipboard);
    if (paths.length > 0) return { paths, temporary: false };
    const image = clipboard.readImage();
    if (image.isEmpty()) return { paths: [], temporary: false };
    const directory = join(tmpdir(), 'picto-clipboard-imports');
    mkdirSync(directory, { recursive: true });
    const path = join(directory, `${randomUUID()}.png`);
    writeFileSync(path, image.toPNG());
    return { paths: [path], temporary: true };
  });

  handle('picto:reverseImageSearch', async (_event, { filePath, engine }) => {
    return runReverseImageSearch({ BrowserWindow, filePath, engine });
  });

  handle('picto:siteIcon:get', async (_event, { domain }) => siteIconService.get(domain));

  ipcMain.on('ondragstart', async (event, { hashes, iconDataUrl }) => {
    if (!windowManager.ownsWebContents(event.sender)) return;
    if (!hashes?.length) return;


    // Drag payloads carry physical file hashes, never logical item identities.
    let filePaths;
    try {
      const resolved = await invoke('media.resolve_paths', { file_hashes: hashes });
      filePaths = resolved.map((entry) => entry.path);
    } catch (err) {
      console.error('[drag:start] resolve failed:', err);
      filePaths = [];
    }
    if (!filePaths?.length) return;

    // Icon: prefer renderer-generated base64 (has count badge), fall back to disk thumbnail
    let icon = null;
    if (iconDataUrl) {
      try {
        icon = nativeImage.createFromDataURL(iconDataUrl);
        if (icon.isEmpty()) icon = null;
      } catch { icon = null; }
    }
    if (!icon) {
      const firstPath = filePaths[0] || '';
      const firstHash = firstPath.split('/').pop()?.replace(/\.[^.]+$/, '') || hashes[0];
      const thumbPath = buildBlobPath('thumb', firstHash, 'jpg');
      try {
        icon = nativeImage.createFromPath(thumbPath);
        if (!icon.isEmpty()) icon = icon.resize({ width: 64 });
        else icon = null;
      } catch { icon = null; }
    }

    if (process.platform === 'darwin' && startNativeDrag) {
      // macOS: bypass Electron's startDrag to avoid icon-per-NSDraggingItem stacking.
      // Native addon gives item[0] the composite icon, items[1-N] get 1×1 transparent.
      const win = BrowserWindow.fromWebContents(event.sender);
      if (win && icon) {
        const handle = win.getNativeWindowHandle();
        const { width, height } = icon.getSize();
        const rgba = Buffer.from(icon.toBitmap()); // RGBA pixel buffer
        // Swap R↔B for macOS native addon (expects BGRA)
        for (let i = 0; i < rgba.length; i += 4) {
          const r = rgba[i];
          rgba[i] = rgba[i + 2];
          rgba[i + 2] = r;
        }
        try {
          startNativeDrag(handle, filePaths, rgba, width, height);
        } catch (err) {
          console.error('[drag:start] native addon failed:', err);
          event.sender.startDrag({ files: filePaths, icon });
        }
      } else {
        event.sender.startDrag({
          files: filePaths,
          icon: icon || nativeImage.createEmpty(),
        });
      }
    } else {
      // Windows / Linux: Electron's startDrag shows one icon for the whole drag.
      event.sender.startDrag({
        files: filePaths,
        icon: icon || nativeImage.createEmpty(),
      });
    }

  });

  const serializeMenu = (menu, pathPrefix = '') => (menu?.items ?? [])
    .filter((item) => item.visible !== false)
    .map((item, index) => {
      const id = pathPrefix ? `${pathPrefix}.${index}` : String(index);
      return {
        id,
        label: item.label,
        type: item.type,
        enabled: item.enabled,
        checked: item.checked,
        accelerator: item.accelerator ?? null,
        submenu: item.submenu ? serializeMenu(item.submenu, id) : null,
      };
    });

  const resolveMenuItem = (id) => {
    const indexes = String(id).split('.').map(Number);
    let menu = Menu.getApplicationMenu();
    let item = null;
    for (const index of indexes) {
      if (!menu || !Number.isInteger(index)) return null;
      item = menu.items[index] ?? null;
      menu = item?.submenu ?? null;
    }
    return item;
  };

  handle('picto:application-menu:get', () => {
    const menu = Menu.getApplicationMenu();
    const items = serializeMenu(menu);
    // The macOS app-name menu contains OS-owned Services/Hide commands.
    // Their Picto equivalents already live in File/Help; the in-window menu
    // exposes the complete cross-platform application command surface.
    return process.platform === 'darwin' && items[0]?.label === app.name ? items.slice(1) : items;
  });

  handle('picto:application-menu:execute', (event, { id }) => {
    const item = resolveMenuItem(id);
    const win = BrowserWindow.fromWebContents(event.sender);
    if (!item || !win || item.enabled === false || item.type === 'separator' || item.submenu) return null;
    if (typeof item.click === 'function') {
      item.click(item, win, {});
      return null;
    }
    const role = String(item.role ?? '').toLowerCase();
    switch (role) {
      case 'cut': win.webContents.cut(); break;
      case 'copy': win.webContents.copy(); break;
      case 'paste': win.webContents.paste(); break;
      case 'selectall': win.webContents.selectAll(); break;
      case 'reload': win.webContents.reload(); break;
      case 'forcereload': win.webContents.reloadIgnoringCache(); break;
      case 'toggledevtools': win.webContents.toggleDevTools(); break;
      case 'togglefullscreen': win.setFullScreen(!win.isFullScreen()); break;
      case 'minimize': win.minimize(); break;
      case 'zoom': win.isMaximized() ? win.unmaximize() : win.maximize(); break;
      case 'close': win.close(); break;
      case 'quit': app.quit(); break;
      default: break;
    }
    return null;
  });

  handle('picto:monitor:current', () => {
    const point = screen.getCursorScreenPoint();
    const display = screen.getDisplayNearestPoint(point);
    return {
      scaleFactor: display.scaleFactor,
      size: { width: display.size.width, height: display.size.height },
    };
  });

  handle('picto:monitor:gpu', async () => {
    const featureStatus = app.getGPUFeatureStatus();
    let info = null;
    try {
      info = await app.getGPUInfo('basic');
    } catch {
      info = null;
    }
    return {
      hardwareAccelerationEnabled: app.isHardwareAccelerationEnabled(),
      featureStatus,
      info,
      experimentalFlagsEnabled: process.env.PICTO_EXPERIMENTAL_GPU_FLAGS === '1',
    };
  });

  handle('picto:library:create', async (_event, payload) => libraryService.createLibrary(payload));
  handle('picto:library:joinCloud', async (_event, payload) => libraryService.joinCloudLibrary(payload));
  handle('picto:library:open', async () => libraryService.openLibraryDialog());
  handle('picto:library:switch', async (_event, { path }) => libraryService.switchLibrary(path));
  handle('picto:library:remove', async (_event, { path }) => libraryService.removeLibrary(path));
  handle('picto:library:delete', async (_event, { path }) => libraryService.deleteLibrary(path));
  handle('picto:library:togglePin', async (_event, { path }) => libraryService.toggleLibraryPin(path));
  handle('picto:library:rename', async (_event, { path, newName }) => libraryService.renameLibrary(path, newName));
  handle('picto:library:relocate', async (_event, { oldPath }) => libraryService.relocateLibrary(oldPath));
  handle('picto:library:getConfig', async () => libraryService.getLibraryConfig());
  handle('picto:library:setMeta', async (_event, { path, meta }) => libraryService.setLibraryMeta(path, meta));
  handle('picto:tutorial:start', async () => libraryService.startTutorialLibrary());
  handle('picto:tutorial:reset', async () => libraryService.resetTutorialLibrary());
  handle('picto:tutorial:finish', async () => libraryService.finishTutorialLibrary());
  handle('picto:tutorial:status', async () => libraryService.getTutorialSession());

  // Auto-updater
  handle('picto:updater:check', async () => {
    try {
      const result = await updaterService.checkForUpdates();
      return result?.updateInfo ?? null;
    } catch (err) {
      return { error: err?.message ?? String(err) };
    }
  });
  handle('picto:updater:download', async () => {
    await updaterService.downloadUpdate();
  });
  handle('picto:updater:install', () => {
    updaterService.quitAndInstall();
  });

  // Shell operations — reveal in Finder/Explorer, open with default app
  handle('picto:shell:showInFolder', async (_event, { path }) => {
    if (path) shell.showItemInFolder(path);
  });

  handle('picto:shell:openPath', async (_event, { path }) => {
    if (path) await shell.openPath(path);
  });

  handle('picto:shell:getOpenWithOptions', async (_event, { path }) => {
    if (!path) throw new Error('A file path is required');
    if (process.platform === 'darwin') {
      const dot = path.lastIndexOf('.');
      const extension = dot >= 0 ? path.slice(dot).toLowerCase() : path.toLowerCase();
      if (!openWithOptionsByExtension.has(extension)) {
        openWithOptionsByExtension.set(extension, {
          mode: 'submenu',
          applications: await getAssociatedApplications(path),
        });
      }
      return openWithOptionsByExtension.get(extension);
    }
    if (process.platform === 'win32') {
      return { mode: 'chooser', applications: [] };
    }
    return { mode: 'unsupported', applications: [] };
  });

  handle('picto:shell:openWithApplication', async (_event, { path, applicationPath }) => {
    if (!path || !applicationPath) throw new Error('A file and application path are required');
    if (process.platform !== 'darwin') throw new Error('Application selection is only available on macOS');
    openWithApplication(applicationPath, path);
  });

  handle('picto:shell:openWithChooser', async (_event, { path }) => {
    if (!path) throw new Error('A file path is required');
    if (process.platform !== 'win32') throw new Error('The system application chooser is unavailable');
    const child = spawn('rundll32.exe', ['shell32.dll,OpenAs_RunDLL', path], {
      detached: true,
      stdio: 'ignore',
      windowsHide: false,
    });
    child.unref();
  });
}
