import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs/promises';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

const AUTH_TIMEOUT_MS = 15 * 60 * 1000;
const TARGET_POLL_MS = 500;
const ONLYFANS_COOKIE_NAMES = new Set(['sess', 'auth_id', 'auth_uid']);

function headerValue(headers, name) {
  const entry = Object.entries(headers ?? {}).find(([key]) => key.toLowerCase() === name);
  return entry == null ? null : String(entry[1] ?? '').trim() || null;
}

export function parseCookieHeader(value) {
  const cookies = {};
  for (const segment of String(value ?? '').split(';')) {
    const separator = segment.indexOf('=');
    if (separator <= 0) continue;
    const name = segment.slice(0, separator).trim();
    const cookieValue = segment.slice(separator + 1).trim();
    if (ONLYFANS_COOKIE_NAMES.has(name) && cookieValue) cookies[name] = cookieValue;
  }
  return cookies;
}

export function createManualOnlyFansCredential({ cookie, user_agent: userAgent, x_bc: xBc }) {
  const cookies = parseCookieHeader(cookie);
  const missingCookies = ['sess', 'auth_id'].filter((name) => !cookies[name]);
  if (missingCookies.length > 0) {
    throw new Error(`OnlyFans Cookie is missing ${missingCookies.join(' and ')}.`);
  }
  const normalizedUserAgent = String(userAgent ?? '').trim();
  const normalizedXBc = String(xBc ?? '').trim();
  if (!normalizedUserAgent) throw new Error('OnlyFans User-Agent is required.');
  if (!normalizedXBc) throw new Error('OnlyFans X-BC is required.');
  return {
    site_category: 'onlyfans',
    credential_type: 'cookies',
    cookies,
    headers: { 'user-agent': normalizedUserAgent, 'x-bc': normalizedXBc },
  };
}

function pathCandidates(platform = process.platform, env = process.env) {
  if (platform === 'darwin') {
    return [
      '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
      '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
      '/Applications/Brave Browser.app/Contents/MacOS/Brave Browser',
      '/Applications/Chromium.app/Contents/MacOS/Chromium',
    ];
  }
  if (platform === 'win32') {
    const roots = [env.LOCALAPPDATA, env.PROGRAMFILES, env['PROGRAMFILES(X86)']].filter(Boolean);
    return roots.flatMap((root) => [
      path.join(root, 'Google', 'Chrome', 'Application', 'chrome.exe'),
      path.join(root, 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
      path.join(root, 'BraveSoftware', 'Brave-Browser', 'Application', 'brave.exe'),
      path.join(root, 'Chromium', 'Application', 'chrome.exe'),
    ]);
  }
  const commands = ['google-chrome-stable', 'google-chrome', 'microsoft-edge', 'brave-browser', 'chromium', 'chromium-browser'];
  return commands.map((command) => spawnSync('which', [command], { encoding: 'utf8' }).stdout.trim()).filter(Boolean);
}

export async function findExternalChromium() {
  for (const candidate of pathCandidates()) {
    try {
      await fs.access(candidate);
      return candidate;
    } catch {}
  }
  throw new Error('OnlyFans Google login requires Chrome, Edge, Brave, or Chromium.');
}

async function availablePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      server.close(() => resolve(address.port));
    });
  });
}

async function readTargets(port) {
  try {
    const response = await fetch(`http://127.0.0.1:${port}/json/list`);
    return response.ok ? response.json() : [];
  } catch {
    return [];
  }
}

function openCdp(url, onEvent) {
  const socket = new WebSocket(url);
  let nextId = 1;
  const pending = new Map();
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.id != null) {
      const request = pending.get(message.id);
      if (!request) return;
      pending.delete(message.id);
      if (message.error) request.reject(new Error(message.error.message));
      else request.resolve(message.result);
      return;
    }
    if (message.method) onEvent(message.method, message.params ?? {});
  };
  const opened = new Promise((resolve, reject) => {
    socket.onopen = resolve;
    socket.onerror = () => reject(new Error('Could not inspect the external login browser.'));
  });
  return {
    async send(method, params = {}) {
      await opened;
      const id = nextId++;
      const response = new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
      socket.send(JSON.stringify({ id, method, params }));
      return response;
    },
    close() {
      for (const request of pending.values()) request.reject(new Error('Browser inspection closed.'));
      pending.clear();
      socket.close();
    },
  };
}

export async function launchExternalOnlyFansAuth({ onStatus = () => {} } = {}) {
  const executable = await findExternalChromium();
  const port = await availablePort();
  const profile = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-onlyfans-auth-'));
  const child = spawn(executable, [
    `--remote-debugging-port=${port}`,
    '--remote-debugging-address=127.0.0.1',
    `--user-data-dir=${profile}`,
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-sync',
    'https://onlyfans.com/',
  ], { stdio: 'ignore' });
  const childExited = new Promise((resolve) => child.once('exit', resolve));

  let closed = false;
  let pollTimer = null;
  let timeout = null;
  const clients = new Map();
  let capturedHeaders = null;
  let resolveCompletion;
  let rejectCompletion;
  const completion = new Promise((resolve, reject) => {
    resolveCompletion = resolve;
    rejectCompletion = reject;
  });

  async function close() {
    if (closed) return;
    closed = true;
    if (pollTimer != null) clearInterval(pollTimer);
    if (timeout != null) clearTimeout(timeout);
    for (const client of clients.values()) client.close();
    clients.clear();
    if (child.exitCode == null) {
      child.kill();
      await Promise.race([
        childExited,
        new Promise((resolve) => setTimeout(resolve, 2000)),
      ]);
    }
    await fs.rm(profile, { recursive: true, force: true }).catch(() => {});
  }

  async function inspectCookies(client) {
    const result = await client.send('Network.getAllCookies');
    const cookies = Object.fromEntries((result.cookies ?? [])
      .filter((cookie) => /(^|\.)onlyfans\.com$/i.test(cookie.domain))
      .filter((cookie) => ONLYFANS_COOKIE_NAMES.has(cookie.name) && cookie.value)
      .map((cookie) => [cookie.name, cookie.value]));
    if (!cookies.sess || !cookies.auth_id || !capturedHeaders) return;
    resolveCompletion({
      site_category: 'onlyfans',
      credential_type: 'cookies',
      cookies,
      headers: capturedHeaders,
    });
  }

  async function attach(target) {
    if (clients.has(target.id) || !target.webSocketDebuggerUrl) return;
    let client;
    const onlyFansRequests = new Set();
    client = openCdp(target.webSocketDebuggerUrl, (method, params) => {
      let headers = null;
      if (method === 'Network.requestWillBeSent') {
        let requestUrl;
        try { requestUrl = new URL(params.request?.url); } catch { return; }
        if (requestUrl.hostname !== 'onlyfans.com' || !requestUrl.pathname.startsWith('/api2/')) return;
        onlyFansRequests.add(params.requestId);
        headers = params.request?.headers;
      } else if (method === 'Network.requestWillBeSentExtraInfo' && onlyFansRequests.has(params.requestId)) {
        headers = params.headers;
      } else {
        return;
      }
      const xBc = headerValue(headers, 'x-bc');
      const userAgent = headerValue(headers, 'user-agent');
      if (xBc && userAgent) capturedHeaders = { 'x-bc': xBc, 'user-agent': userAgent };
      void inspectCookies(client).catch(() => {});
    });
    clients.set(target.id, client);
    await client.send('Network.enable');
    await inspectCookies(client);
  }

  async function poll() {
    if (closed) return;
    const targets = await readTargets(port);
    for (const target of targets) {
      if (target.type === 'page') await attach(target).catch(() => {});
    }
  }

  child.once('error', (error) => {
    rejectCompletion(new Error(`Could not open the external login browser: ${error.message}`));
    void close();
  });
  child.once('exit', () => {
    if (!closed) rejectCompletion(new Error('OnlyFans login was closed before authentication completed.'));
    void close();
  });
  timeout = setTimeout(() => {
    rejectCompletion(new Error('OnlyFans login timed out.'));
    void close();
  }, AUTH_TIMEOUT_MS);
  pollTimer = setInterval(() => { void poll(); }, TARGET_POLL_MS);
  onStatus('Complete the OnlyFans login in the external browser.');
  void poll();
  return { completion, close };
}
