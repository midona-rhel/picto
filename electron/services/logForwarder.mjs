/**
 * Forward log messages from the Electron main process to the renderer's
 * log store via IPC. This ensures all logs (media protocol 404s, native
 * addon messages, etc.) appear in the in-app log viewer.
 */

/** @type {import('electron').BrowserWindow | null} */
let _mainWindow = null;

export function setMainWindow(win) {
  _mainWindow = win;
}

/**
 * Send a log entry to the renderer.
 * @param {'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR'} level
 * @param {string} target - module/source identifier
 * @param {string} message
 */
export function forwardLog(level, target, message) {
  if (!_mainWindow || _mainWindow.isDestroyed()) return;
  _mainWindow.webContents.send('picto:log', {
    level,
    target,
    message,
    timestamp: new Date().toISOString(),
  });
}

export function forwardWarn(target, message) {
  forwardLog('WARN', target, message);
}

export function forwardError(target, message) {
  forwardLog('ERROR', target, message);
}

export function forwardInfo(target, message) {
  forwardLog('INFO', target, message);
}
