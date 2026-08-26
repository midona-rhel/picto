import fs from 'node:fs/promises';

const wait = (duration) => new Promise((resolve) => setTimeout(resolve, duration));

async function waitForReady(webContents, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const state = await webContents.executeJavaScript('window.__pictoPdfThumbnail ?? null', true);
    if (state?.error) throw new Error(state.error);
    if (state?.ready) return state;
    await wait(50);
  }
  throw new Error('PDF thumbnail rendering timed out.');
}

export function createPdfThumbnailService({ BrowserWindow, app, path, isDev, devUrl }) {
  let queue = Promise.resolve();

  async function renderNow({ sourceUrl, outputPath }) {
    const window = new BrowserWindow({
      show: false,
      width: 800,
      height: 800,
      useContentSize: true,
      paintWhenInitiallyHidden: true,
      webPreferences: {
        backgroundThrottling: false,
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
      },
    });
    try {
      const query = { src: sourceUrl };
      if (isDev) await window.loadURL(`${devUrl}/pdf-thumbnail.html?${new URLSearchParams(query)}`);
      else await window.loadFile(path.join(app.getAppPath(), 'dist', 'pdf-thumbnail.html'), { query });
      const state = await waitForReady(window.webContents);
      const dataUrl = await window.webContents.executeJavaScript(
        `document.querySelector('#page')?.toDataURL('image/png') ?? ''`,
        true,
      );
      if (!dataUrl.startsWith('data:image/png;base64,')) {
        throw new Error('PDF thumbnail canvas did not produce PNG data.');
      }
      const png = Buffer.from(dataUrl.slice('data:image/png;base64,'.length), 'base64');
      if (png.length === 0) throw new Error('PDF thumbnail capture was empty.');
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
