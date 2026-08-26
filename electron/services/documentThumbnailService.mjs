import fs from 'node:fs/promises';

const wait = (duration) => new Promise((resolve) => setTimeout(resolve, duration));

async function waitForReady(webContents, timeoutMs = 25_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const state = await webContents.executeJavaScript('window.__pictoDocumentThumbnail ?? null', true);
    if (state?.error) throw new Error(state.error);
    if (state?.ready) return state;
    await wait(50);
  }
  throw new Error('Document thumbnail rendering timed out.');
}

export function createDocumentThumbnailService({ BrowserWindow, app, path, isDev, devUrl }) {
  let queue = Promise.resolve();

  async function renderNow({ hash, mimeType, outputPath }) {
    const window = new BrowserWindow({
      show: false,
      width: 1328,
      height: 1200,
      useContentSize: true,
      paintWhenInitiallyHidden: true,
      backgroundColor: '#2a2a2e',
      webPreferences: {
        backgroundThrottling: false,
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
      },
    });
    window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    window.webContents.on('will-navigate', (event) => event.preventDefault());
    try {
      const query = { hash, mime: mimeType };
      if (isDev) await window.loadURL(`${devUrl}/document-thumbnail.html?${new URLSearchParams(query)}`);
      else await window.loadFile(path.join(app.getAppPath(), 'dist', 'document-thumbnail.html'), { query });
      const state = await waitForReady(window.webContents);
      const png = (await window.webContents.capturePage({
        x: state.x,
        y: state.y,
        width: state.width,
        height: state.height,
      })).toPNG();
      if (png.length === 0) throw new Error('Document thumbnail capture was empty.');
      await fs.mkdir(path.dirname(outputPath), { recursive: true });
      await fs.writeFile(outputPath, png);
      return { width: state.width, height: state.height };
    } finally {
      if (!window.isDestroyed()) window.destroy();
    }
  }

  return {
    render(options) {
      const result = queue.then(() => renderNow(options));
      queue = result.catch(() => {});
      return result;
    },
  };
}
