import fs from 'node:fs/promises';

export const FLASH_THUMBNAIL_SETTLE_MS = 5_000;
const LOAD_TIMEOUT_MS = 15_000;
const READY_POLL_MS = 50;

const wait = (duration) => new Promise((resolve) => setTimeout(resolve, duration));

function fitSize(width, height, maxWidth, maxHeight) {
  if (!(width > 0) || !(height > 0)) return { width: maxWidth, height: maxHeight };
  const scale = Math.min(maxWidth / width, maxHeight / height);
  return {
    width: Math.max(1, Math.round(width * scale)),
    height: Math.max(1, Math.round(height * scale)),
  };
}

async function waitForReady(webContents, timeoutMs = LOAD_TIMEOUT_MS) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const state = await webContents.executeJavaScript('window.__pictoFlashThumbnail ?? null', true);
    if (state?.error) throw new Error(state.error);
    if (state?.ready) return state;
    await wait(READY_POLL_MS);
  }
  throw new Error('Flash thumbnail rendering timed out.');
}

export function createFlashThumbnailService({ BrowserWindow, app, path, isDev, devUrl }) {
  let queue = Promise.resolve();

  async function captureNow({ sourceUrl, maxWidth = 800, maxHeight = 800, settleMs = FLASH_THUMBNAIL_SETTLE_MS }) {
    const window = new BrowserWindow({
      show: false,
      width: maxWidth,
      height: maxHeight,
      useContentSize: true,
      paintWhenInitiallyHidden: true,
      webPreferences: {
        backgroundThrottling: false,
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
      },
    });
    window.webContents.setAudioMuted(true);

    try {
      const query = { src: sourceUrl };
      if (isDev) {
        await window.loadURL(`${devUrl}/flash-thumbnail.html?${new URLSearchParams(query)}`);
      } else {
        await window.loadFile(path.join(app.getAppPath(), 'dist', 'flash-thumbnail.html'), { query });
      }

      const state = await waitForReady(window.webContents);
      const size = fitSize(state.width, state.height, maxWidth, maxHeight);
      window.setContentSize(size.width, size.height, false);
      await wait(settleMs);

      const png = (await window.webContents.capturePage()).toPNG();
      if (png.length === 0) throw new Error('Flash thumbnail capture was empty.');
      return { width: size.width, height: size.height, png };
    } finally {
      if (!window.isDestroyed()) window.destroy();
    }
  }

  async function renderNow(options) {
    const result = await captureNow(options);
    await fs.mkdir(path.dirname(options.outputPath), { recursive: true });
    await fs.writeFile(options.outputPath, result.png);
    return { width: result.width, height: result.height };
  }

  function enqueue(task) {
    const result = queue.then(task);
    queue = result.catch(() => {});
    return result;
  }

  return {
    render(options) {
      return enqueue(() => renderNow(options));
    },
  };
}

export { fitSize };
