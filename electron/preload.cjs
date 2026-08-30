const { contextBridge, ipcRenderer, webUtils } = require('electron');

if (process.env.PICTO_PACKAGED_SMOKE === '1') {
  const reportRendererFailure = (event, message) => {
    ipcRenderer.send('picto:smoke:renderer-failure', { event, message });
  };
  window.addEventListener('error', (event) => {
    reportRendererFailure('window-error', event.error?.message ?? event.message ?? 'renderer error');
  });
  window.addEventListener('unhandledrejection', (event) => {
    reportRendererFailure('unhandled-rejection', event.reason?.message ?? String(event.reason));
  });
}

function on(channel, handler) {
  const listener = (_event, payload) => handler(payload);
  ipcRenderer.on(channel, listener);
  return () => ipcRenderer.removeListener(channel, listener);
}

const api = {
  invoke: (command, args = {}) => ipcRenderer.invoke('picto:invoke', { command, args }),
  window: {
    call: (method, payload = {}) => ipcRenderer.invoke('picto:window', { method, payload }),
  },
  getApplicationMenu: () => ipcRenderer.invoke('picto:application-menu:get'),
  executeApplicationMenuItem: (id) => ipcRenderer.invoke('picto:application-menu:execute', { id }),
  setApplicationMenuShortcuts: (bindings) => ipcRenderer.invoke('picto:application-menu:set-shortcuts', { bindings }),
  setApplicationMenuContext: (context) => ipcRenderer.invoke('picto:application-menu:set-context', { context }),
  restartMainWindow: () => ipcRenderer.invoke('picto:restart-main-window'),
};

const windowControls = {
  minimize: () => ipcRenderer.send('picto:window-control', 'minimize'),
  toggleMaximize: () => ipcRenderer.send('picto:window-control', 'toggleMaximize'),
  close: () => ipcRenderer.send('picto:window-control', 'close'),
};

const events = {
  on: (name, handler) => Promise.resolve(on(name, handler)),
  emit: (name, payload) => ipcRenderer.invoke('picto:event:emit', { name, payload, target: null }),
  emitTo: (target, name, payload) => ipcRenderer.invoke('picto:event:emit', { name, payload, target }),
};

const dialog = {
  open: (options = {}) => ipcRenderer.invoke('picto:dialog:open', options),
  save: (options = {}) => ipcRenderer.invoke('picto:dialog:save', options),
};

const diagnostics = {
  save: (content) => ipcRenderer.invoke('picto:diagnostics:save', { content }),
};

const clipboard = {
  writeText: (text) => ipcRenderer.invoke('picto:clipboard:writeText', { text }),
  copyFile: (filePath) => ipcRenderer.invoke('picto:clipboard:copyFile', { filePath }),
  copyFiles: (filePaths) => ipcRenderer.invoke('picto:clipboard:copyFiles', { filePaths }),
  copyImage: (filePath) => ipcRenderer.invoke('picto:clipboard:copyImage', { filePath }),
  hasImport: () => ipcRenderer.invoke('picto:clipboard:hasImport'),
  readImport: () => ipcRenderer.invoke('picto:clipboard:readImport'),
};

const shellOps = {
  showInFolder: (path) => ipcRenderer.invoke('picto:shell:showInFolder', { path }),
  openPath: (path) => ipcRenderer.invoke('picto:shell:openPath', { path }),
  getOpenWithOptions: (path) => ipcRenderer.invoke('picto:shell:getOpenWithOptions', { path }),
  openWithApplication: (path, applicationPath) => ipcRenderer.invoke('picto:shell:openWithApplication', { path, applicationPath }),
  openWithChooser: (path) => ipcRenderer.invoke('picto:shell:openWithChooser', { path }),
};

const search = {
  reverseImage: (filePath, engine) => ipcRenderer.invoke('picto:reverseImageSearch', { filePath, engine }),
};

const siteIcons = {
  get: (domain) => ipcRenderer.invoke('picto:siteIcon:get', { domain }),
};

const library = {
  create: (name, savePath) => ipcRenderer.invoke('picto:library:create', { name, savePath }),
  joinCloud: (input) => ipcRenderer.invoke('picto:library:joinCloud', input),
  open: () => ipcRenderer.invoke('picto:library:open'),
  switch: (path) => ipcRenderer.invoke('picto:library:switch', { path }),
  remove: (path) => ipcRenderer.invoke('picto:library:remove', { path }),
  delete: (path) => ipcRenderer.invoke('picto:library:delete', { path }),
  togglePin: (path) => ipcRenderer.invoke('picto:library:togglePin', { path }),
  getConfig: () => ipcRenderer.invoke('picto:library:getConfig'),
  rememberCloudRoot: (root) => ipcRenderer.invoke('picto:library:rememberCloudRoot', root),
  rename: (path, newName) => ipcRenderer.invoke('picto:library:rename', { path, newName }),
  relocate: (oldPath) => ipcRenderer.invoke('picto:library:relocate', { oldPath }),
  setMeta: (path, meta) => ipcRenderer.invoke('picto:library:setMeta', { path, meta }),
};

const tutorial = {
  start: () => ipcRenderer.invoke('picto:tutorial:start'),
  reset: () => ipcRenderer.invoke('picto:tutorial:reset'),
  finish: () => ipcRenderer.invoke('picto:tutorial:finish'),
  status: () => ipcRenderer.invoke('picto:tutorial:status'),
};

const monitor = {
  current: () => ipcRenderer.invoke('picto:monitor:current'),
  gpu: () => ipcRenderer.invoke('picto:monitor:gpu'),
};

const updates = {
  check: () => ipcRenderer.invoke('picto:updates:check'),
  state: () => ipcRenderer.invoke('picto:updates:state'),
  install: () => ipcRenderer.invoke('picto:updates:install'),
  openRelease: () => ipcRenderer.invoke('picto:updates:open-release'),
  onState: (handler) => on('picto:update-state', handler),
};

const webview = {
  startNativeDrag: (hashes, iconDataUrl) => ipcRenderer.send('ondragstart', { hashes, iconDataUrl }),
  onDragDropEvent: (handler) => {
    // Counter tracks nested dragenter/dragleave from child elements.
    let dragCounter = 0;

    const dragEnter = (e) => {
      e.preventDefault();
      dragCounter++;
      if (dragCounter === 1) {
        handler({ payload: { type: 'enter' } });
      }
    };
    const dragLeave = (e) => {
      e.preventDefault();
      dragCounter--;
      if (dragCounter <= 0) {
        dragCounter = 0;
        handler({ payload: { type: 'leave' } });
      }
    };
    const dragOver = (e) => {
      e.preventDefault();
    };
    const drop = (e) => {
      e.preventDefault();
      dragCounter = 0;
      const files = Array.from(e.dataTransfer?.files ?? []);
      const paths = files.map((f) => webUtils.getPathForFile(f)).filter(Boolean);
      handler({ payload: { type: 'drop', paths } });
    };

    window.addEventListener('dragenter', dragEnter);
    window.addEventListener('dragleave', dragLeave);
    window.addEventListener('dragover', dragOver);
    window.addEventListener('drop', drop);

    return Promise.resolve(() => {
      window.removeEventListener('dragenter', dragEnter);
      window.removeEventListener('dragleave', dragLeave);
      window.removeEventListener('dragover', dragOver);
      window.removeEventListener('drop', drop);
    });
  },
};

contextBridge.exposeInMainWorld('picto', {
  api,
  windowControls,
  events,
  dialog,
  diagnostics,
  clipboard,
  shell: shellOps,
  monitor,
  updates,
  webview,
  search,
  siteIcons,
  library,
  tutorial,
});
