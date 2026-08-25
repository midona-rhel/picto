/**
 * Forward log messages from the Electron main process to the renderer's
 * log store via IPC. This ensures all logs (media protocol 404s, native
 * addon messages, etc.) appear in the in-app log viewer.
 */

import { inspect } from 'node:util';

/** @type {import('electron').BrowserWindow | null} */
let _mainWindow = null;
const pending = [];
let consoleInstalled = false;

export function setMainWindow(win) {
  _mainWindow = win;
  for (const entry of pending.splice(0)) send(entry);
}

function send(entry) {
  if (!_mainWindow || _mainWindow.isDestroyed()) {
    pending.push(entry);
    if (pending.length > 500) pending.shift();
    return;
  }
  _mainWindow.webContents.send('picto:log', entry);
}

/**
 * Send a log entry to the renderer.
 * @param {'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR'} level
 * @param {string} target - module/source identifier
 * @param {string} message
 */
export function forwardLog(level, target, message) {
  send({
    level,
    target,
    message,
    timestamp: new Date().toISOString(),
  });
}

export function installConsoleForwarding() {
  if (consoleInstalled) return;
  consoleInstalled = true;
  const levels = { debug: 'DEBUG', info: 'INFO', log: 'INFO', warn: 'WARN', error: 'ERROR' };
  for (const [method, level] of Object.entries(levels)) {
    const original = console[method].bind(console);
    console[method] = (...args) => {
      original(...args);
      const message = args
        .map((value) => typeof value === 'string' ? value : inspect(value, { depth: 3 }))
        .join(' ');
      forwardLog(level, 'electron', message);
    };
  }
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
