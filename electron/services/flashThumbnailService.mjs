import fs from 'node:fs/promises';
import { setTimeout as wait } from 'node:timers/promises';
import { forwardError } from './logForwarder.mjs';

export const FLASH_THUMBNAIL_SETTLE_MS = 5_000;
export const FLASH_THUMBNAIL_TIMEOUT_MS = 20_000;
const READY_POLL_MS = 50;

function fitSize(width, height, maxWidth, maxHeight) {
  if (!(width > 0) || !(height > 0)) return { width: maxWidth, height: maxHeight };
  const scale = Math.min(maxWidth / width, maxHeight / height);
  return {
    width: Math.max(1, Math.round(width * scale)),
    height: Math.max(1, Math.round(height * scale)),
  };
}

async function waitForReady(webContents, signal) {
  while (!signal.aborted) {
    const state = await webContents.executeJavaScript('window.__pictoFlashThumbnail ?? null', true);
    signal.throwIfAborted();
    if (state?.error) throw new Error(state.error);
    if (state?.ready) return state;
    await wait(READY_POLL_MS, undefined, { signal });
  }
  throw new Error('Flash thumbnail rendering timed out.');
}

export function createFlashThumbnailService({ BrowserWindow, app, path, isDev, devUrl, timeoutMs = FLASH_THUMBNAIL_TIMEOUT_MS }) {
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
    const controller = new AbortController();
    const { signal } = controller;
    let timer;
    const timeout = new Promise((_, reject) => {
      timer = setTimeout(() => {
        const error = new Error('Flash thumbnail rendering timed out.');
        controller.abort(error);
        reject(error);
      }, timeoutMs);
    });
    const capture = async () => {
      window.webContents.setAudioMuted(true);
      window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
      const query = { src: sourceUrl };
      if (isDev) {
        await window.loadURL(`${devUrl}/flash-thumbnail.html?${new URLSearchParams(query)}`);
      } else {
        await window.loadFile(path.join(app.getAppPath(), 'dist', 'flash-thumbnail.html'), { query });
      }
      signal.throwIfAborted();
      const state = await waitForReady(window.webContents, signal);
      signal.throwIfAborted();
      const size = fitSize(state.width, state.height, maxWidth, maxHeight);
      window.setContentSize(size.width, size.height, false);
      await wait(settleMs, undefined, { signal });

      const png = (await window.webContents.capturePage()).toPNG();
      signal.throwIfAborted();
      if (png.length === 0) throw new Error('Flash thumbnail capture was empty.');
      return { width: size.width, height: size.height, png };
    };
    try {
      return await Promise.race([capture(), timeout]);
    } finally {
      clearTimeout(timer);
      controller.abort();
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
      return enqueue(() => renderNow(options)).catch((error) => {
        forwardError('flash.thumbnail', `${options.sourceUrl}: ${error.message ?? String(error)}`);
        throw error;
      });
    },
  };
}

export { fitSize };
