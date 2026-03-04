const { contextBridge, ipcRenderer, webUtils } = require('electron');

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
  popupMenu: () => ipcRenderer.invoke('picto:popup-menu'),
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

const clipboard = {
  writeText: (text) => ipcRenderer.invoke('picto:clipboard:writeText', { text }),
  copyFile: (filePath) => ipcRenderer.invoke('picto:clipboard:copyFile', { filePath }),
  copyImage: (filePath) => ipcRenderer.invoke('picto:clipboard:copyImage', { filePath }),
};

const search = {
  reverseImage: (filePath, engine) => ipcRenderer.invoke('picto:reverseImageSearch', { filePath, engine }),
};

const library = {
  create: (name, savePath) => ipcRenderer.invoke('picto:library:create', { name, savePath }),
  open: () => ipcRenderer.invoke('picto:library:open'),
  switch: (path) => ipcRenderer.invoke('picto:library:switch', { path }),
  remove: (path) => ipcRenderer.invoke('picto:library:remove', { path }),
  delete: (path) => ipcRenderer.invoke('picto:library:delete', { path }),
  togglePin: (path) => ipcRenderer.invoke('picto:library:togglePin', { path }),
  getConfig: () => ipcRenderer.invoke('picto:library:getConfig'),
  rename: (path, newName) => ipcRenderer.invoke('picto:library:rename', { path, newName }),
  relocate: (oldPath) => ipcRenderer.invoke('picto:library:relocate', { oldPath }),
};

const monitor = {
  current: () => ipcRenderer.invoke('picto:monitor:current'),
};

const webview = {
  startNativeDrag: (hashes, iconDataUrl) => ipcRenderer.invoke('picto:drag:start', { hashes, iconDataUrl }),
  onDragDropEvent: (handler) => {
    // Counter tracks nested dragenter/dragleave from child elements.
    // Without this, dragleave fires when moving between children and
    // the drop overlay disappears prematurely.
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
  events,
  dialog,
  clipboard,
  monitor,
  webview,
  search,
  library,
});
