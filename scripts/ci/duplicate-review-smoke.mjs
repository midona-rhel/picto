#!/usr/bin/env node

import { spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { deflateSync } from 'node:zlib';
import { setTimeout as delay } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';
import { findUnpackedExecutable, normalizePlatform } from './alpha-smoke.mjs';

const DEFAULT_TIMEOUT_MS = 30_000;

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index];
    if (key === '--help') {
      args.help = true;
    } else if (key.startsWith('--')) {
      args[key.slice(2)] = argv[index + 1] ?? true;
      index += 1;
    }
  }
  return args;
}

function usage() {
  console.log(`Usage: node scripts/ci/duplicate-review-smoke.mjs [options]

Runs the duplicate review flow through a packaged Electron renderer's real preload IPC.
The package must already exist; this script does not build it.

Options:
  --dist <dir>          electron-builder output directory (default: dist)
  --executable <path>   packaged executable, bypassing --dist discovery
  --platform <name>     darwin, linux, or win32 (default: current platform)
  --report <path>       JSON report path (default: artifacts/duplicates/smoke.json)
  --timeout <ms>        per wait timeout (default: 30000)
`);
}

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function pngChunk(type, payload) {
  const typeBytes = Buffer.from(type, 'ascii');
  const size = Buffer.alloc(4);
  size.writeUInt32BE(payload.length, 0);
  const checksum = Buffer.alloc(4);
  checksum.writeUInt32BE(crc32(Buffer.concat([typeBytes, payload])), 0);
  return Buffer.concat([size, typeBytes, payload, checksum]);
}

function createPng({ variant = 'same', marker }) {
  const width = 64;
  const height = 64;
  const rows = [];
  for (let y = 0; y < height; y += 1) {
    const row = Buffer.alloc(1 + width * 3);
    for (let x = 0; x < width; x += 1) {
      const offset = 1 + x * 3;
      const checker = ((x >> 3) + (y >> 3)) % 2 === 0;
      const base = variant === 'same'
        ? (checker ? 220 : 40)
        : (x > y ? 190 : 25);
      row[offset] = base;
      row[offset + 1] = variant === 'same' ? (checker ? 120 : 55) : 75;
      row[offset + 2] = variant === 'same' ? (checker ? 55 : 180) : 210;
    }
    rows.push(row);
  }

  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 2;
  const text = Buffer.from(`picto-smoke\0${marker}`, 'latin1');
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    pngChunk('IHDR', ihdr),
    pngChunk('tEXt', text),
    pngChunk('IDAT', deflateSync(Buffer.concat(rows))),
    pngChunk('IEND', Buffer.alloc(0)),
  ]);
}

function waitForProcessClose(child, timeoutMs = 5_000) {
  if (child.exitCode !== null) return Promise.resolve();
  return new Promise((resolve) => {
    let settled = false;
    const finish = () => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      resolve();
    };
    const timeout = setTimeout(() => {
      child.kill('SIGKILL');
      finish();
    }, timeoutMs);
    child.once('close', finish);
  });
}

async function stopProcess(child) {
  if (!child || child.exitCode !== null) return;
  child.kill('SIGTERM');
  await waitForProcessClose(child);
}

async function waitForDebugger(port, timeoutMs, processState) {
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  while (Date.now() < deadline) {
    if (processState.closed) {
      throw new Error(`Electron exited before DevTools endpoint: ${processState.signal ? `signal ${processState.signal}` : `exit code ${processState.code}`}`);
    }
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`);
      if (response.ok) {
        const pages = await response.json();
        const page = pages.find((candidate) => candidate.type === 'page' && candidate.webSocketDebuggerUrl);
        if (page) return page.webSocketDebuggerUrl;
      }
    } catch (error) {
      lastError = error;
    }
    await delay(100);
  }
  throw new Error(`Electron DevTools endpoint did not become available${lastError ? `: ${lastError.message}` : ''}`);
}

function connectCdp(webSocketUrl) {
  const socket = new WebSocket(webSocketUrl);
  const pending = new Map();
  let nextId = 1;
  let connected = false;

  const ready = new Promise((resolve, reject) => {
    socket.addEventListener('open', () => {
      connected = true;
      resolve();
    });
    socket.addEventListener('error', () => reject(new Error('Failed to connect to Electron DevTools')));
  });

  socket.addEventListener('message', (event) => {
    const message = JSON.parse(event.data);
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    if (message.error) request.reject(new Error(message.error.message));
    else request.resolve(message.result);
  });
  socket.addEventListener('close', () => {
    for (const request of pending.values()) request.reject(new Error('Electron DevTools connection closed'));
    pending.clear();
  });

  async function command(method, params = {}) {
    await ready;
    if (!connected) throw new Error('Electron DevTools is not connected');
    const id = nextId++;
    const result = new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
    socket.send(JSON.stringify({ id, method, params }));
    return result;
  }

  async function evaluate(expression) {
    const result = await command('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
      userGesture: true,
    });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description ?? 'Renderer evaluation failed');
    }
    if (result.result?.subtype === 'error') {
      throw new Error(result.result.description ?? 'Renderer evaluation failed');
    }
    return result.result?.value;
  }

  return {
    ready,
    evaluate,
    close() {
      socket.close();
    },
  };
}

async function waitFor(label, read, predicate, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let lastValue;
  while (Date.now() < deadline) {
    lastValue = await read();
    if (predicate(lastValue)) return lastValue;
    await delay(250);
  }
  const error = new Error(`${label} did not settle: ${JSON.stringify(lastValue)}`);
  error.lastValue = lastValue;
  throw error;
}

function invokeExpression(command, args = {}) {
  return `(async () => window.picto.api.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)}))()`;
}

function buildDomExpression(operation, name) {
  const encodedName = JSON.stringify(name);
  const textOperation = operation === 'text' || operation === 'text-click';
  const selector = textOperation
    ? "'*'"
    : "'button,[role=\"button\"],[role=\"region\"],section[aria-label]'";
  const matcher = textOperation
    ? `candidate.textContent?.trim() === target`
    : `((candidate.getAttribute('aria-label') || candidate.textContent || '').trim() === target)`;
  return `(function () {
    const target = ${encodedName};
    const visible = (element) => {
      const style = window.getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0 && rect.height > 0;
    };
    const candidates = Array.from(document.querySelectorAll(${selector}));
    const element = candidates.find((candidate) => visible(candidate) && ${matcher});
    if (!element) return { found: false, name: target };
    const disabled = element instanceof HTMLButtonElement && (element.disabled || element.getAttribute('aria-disabled') === 'true');
    if (${JSON.stringify(operation)} === 'click' || ${JSON.stringify(operation)} === 'text-click' || ${JSON.stringify(operation)} === 'role-click') element.click();
    return { found: true, name: target, disabled, text: element.textContent?.trim() || '' };
  })()`;
}

function buildDomTextContainerExpression(name) {
  return `(function () {
    const target = ${JSON.stringify(name)};
    const visible = (element) => {
      const style = window.getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0 && rect.height > 0;
    };
    const element = Array.from(document.querySelectorAll('*')).find((candidate) => visible(candidate) && candidate.textContent?.trim() === target);
    return element ? { found: true, text: element.parentElement?.textContent?.trim() || target } : { found: false, text: '' };
  })()`;
}

function clickRoleExpression(name) {
  return buildDomExpression('role-click', name);
}

function clickTextExpression(name) {
  return buildDomExpression('text-click', name);
}

function readRoleExpression(name) {
  return buildDomExpression('role', name);
}

function readTextExpression(name) {
  return buildDomExpression('text', name);
}

function sidebarNode(tree, id) {
  return tree?.nodes?.find((node) => node.id === id) ?? null;
}

async function launch(executable, { appData, libraryRoot, port, timeoutMs }) {
  const child = spawn(executable, [`--remote-debugging-port=${port}`, `--user-data-dir=${appData}`], {
    cwd: path.dirname(executable),
    env: {
      ...process.env,
      PICTO_LIBRARY_ROOT: libraryRoot,
      PICTO_E2E: '1',
      PICTO_SMOKE_APP_DATA: appData,
      ELECTRON_NO_ATTACH_CONSOLE: '1',
    },
    windowsHide: true,
  });
  let stderr = '';
  let stdout = '';
  const processState = { closed: false, code: null, signal: null };
  child.once('close', (code, signal) => {
    processState.closed = true;
    processState.code = code;
    processState.signal = signal;
  });
  child.stdout?.on('data', (chunk) => { stdout = `${stdout}${chunk}`.slice(-16_384); });
  child.stderr?.on('data', (chunk) => { stderr = `${stderr}${chunk}`.slice(-16_384); });
  let cdp = null;
  try {
    const webSocketUrl = await waitForDebugger(port, timeoutMs, processState);
    cdp = connectCdp(webSocketUrl);
    await cdp.ready;
    const mainPage = () => cdp.evaluate(`({
      url: location.href,
      ready_state: document.readyState,
      title: document.title,
      body_text: document.body?.innerText?.slice(0, 500) || '',
      preload: typeof window.picto?.api?.invoke === 'function',
    })`);
    await waitFor(
      'renderer main page',
      mainPage,
      (state) => state.url.includes('index.html') && state.ready_state === 'complete' && state.preload,
      timeoutMs,
    ).catch((error) => {
      const state = error?.lastValue;
      throw new Error(`Renderer main page did not load: ${JSON.stringify(state ?? error.message)}`);
    });
    return { child, cdp, getOutput: () => ({ stdout, stderr }) };
  } catch (error) {
    cdp?.close();
    error.launchChild = child;
    error.launchOutput = { stdout, stderr };
    throw error;
  }
}

async function runScenario({ cdp, sourceRoot, libraryRoot, timeoutMs }) {
  const steps = [];
  const runStep = async (name, action) => {
    const started = performance.now();
    try {
      const details = await action();
      steps.push({ name, passed: true, elapsed_ms: Math.round(performance.now() - started), details });
      return details;
    } catch (error) {
      steps.push({ name, passed: false, elapsed_ms: Math.round(performance.now() - started), error: error.message });
      error.smokeSteps = steps;
      throw error;
    }
  };

  const getSidebar = () => cdp.evaluate(invokeExpression('get_sidebar_tree'));
  const getPairs = () => cdp.evaluate(invokeExpression('get_duplicate_pairs', { limit: 20, status: 'detected' }));
  const readUi = () => cdp.evaluate(`({
    duplicates: ${readTextExpression('Duplicates')},
    scan: ${readRoleExpression('Scan library')},
    rescan: ${readRoleExpression('Re-scan library')},
    keepLeft: ${readRoleExpression('Keep left')},
    keepRight: ${readRoleExpression('Keep right')},
    keepBoth: ${readRoleExpression('Keep both')},
    leftCandidate: ${readTextExpression('Left candidate')},
    rightCandidate: ${readTextExpression('Right candidate')},
    noPairs: ${readRoleExpression('No duplicate pairs')},
    reviewComplete: ${readRoleExpression('Review complete')},
  })`);
  const clickText = (text) => cdp.evaluate(clickTextExpression(text));
  const clickRole = (label) => cdp.evaluate(clickRoleExpression(label));

  await runStep('navigate rendered app to Duplicates', async () => {
    await waitFor('Duplicates navigation item', () => cdp.evaluate(readTextExpression('Duplicates')), (value) => value.found, timeoutMs);
    const result = await clickText('Duplicates');
    if (!result.found) throw new Error('Visible Duplicates navigation item was not clickable');
    await waitFor('duplicate empty state', readUi, (ui) => ui.noPairs.found || ui.reviewComplete.found || ui.scan.found, timeoutMs);
    return { navigation: result.text };
  });

  await runStep('import active and inbox fixtures', async () => {
    await cdp.evaluate(invokeExpression('add_media', {
      paths: [path.join(sourceRoot, 'active-a.png'), path.join(sourceRoot, 'active-b.png')],
      initial_status: 1,
      preserve_structure: false,
      parent_folder_id: null,
      collection_name: null,
    }));
    await waitFor('active/inbox counts', getSidebar, (tree) => (
      sidebarNode(tree, 'system:active')?.count === 2
    ), timeoutMs);
    await cdp.evaluate(invokeExpression('add_media', {
      paths: [path.join(sourceRoot, 'inbox.png')],
      initial_status: 0,
      preserve_structure: false,
      parent_folder_id: null,
      collection_name: null,
    }));
    const tree = await waitFor('inbox count', getSidebar, (value) => (
      sidebarNode(value, 'system:active')?.count === 2
      && sidebarNode(value, 'system:inbox')?.count === 1
    ), timeoutMs);
    return {
      all_count: sidebarNode(tree, 'system:active')?.count,
      inbox_count: sidebarNode(tree, 'system:inbox')?.count,
      contract: 'All is active only; Inbox is separate.',
    };
  });

  const scan = await runStep('scan through rendered Scan library control', async () => {
    const control = await waitFor('Scan library control', readUi, (ui) => ui.scan.found && !ui.scan.disabled, timeoutMs);
    const clicked = await clickRole('Scan library');
    if (!clicked.found) throw new Error('Visible Scan library control was not clickable');
    const ui = await waitFor('rendered duplicate candidate cards', readUi, (value) => (
      value.keepLeft.found && !value.keepLeft.disabled
      && value.keepRight.found && !value.keepRight.disabled
      && value.leftCandidate.found && value.rightCandidate.found
    ), timeoutMs);
    const pairs = await getPairs();
    if (pairs.items.length < 1) throw new Error(`Rendered cards appeared without an authoritative candidate: ${JSON.stringify(pairs)}`);
    return { control: control.text, rendered_cards: 2, detected_pairs: pairs.total, ui };
  });

  await runStep('re-scan through rendered Re-scan control', async () => {
    const control = await waitFor('Re-scan library control', readUi, (ui) => ui.rescan.found && !ui.rescan.disabled, timeoutMs);
    const clicked = await clickRole('Re-scan library');
    if (!clicked.found) throw new Error('Visible Re-scan library control was not clickable');
    const ui = await waitFor('candidate cards after rendered re-scan', readUi, (value) => (
      value.rescan.found && !value.rescan.disabled
      && value.keepLeft.found && !value.keepLeft.disabled
      && value.keepRight.found && !value.keepRight.disabled
      && value.leftCandidate.found && value.rightCandidate.found
    ), timeoutMs);
    return { control: control.text, rendered_cards: 2, ui };
  });

  await runStep('resolve through rendered Keep both control', async () => {
    const control = await waitFor('Keep both control', readUi, (ui) => ui.keepBoth.found && !ui.keepBoth.disabled, timeoutMs);
    const clicked = await clickRole('Keep both');
    if (!clicked.found) throw new Error('Visible Keep both control was not clickable');
    const ui = await waitFor('rendered duplicate empty state', readUi, (value) => (
      value.reviewComplete.found || value.noPairs.found
    ), timeoutMs);
    if (ui.reviewComplete.found === false && ui.noPairs.found === false) {
      throw new Error(`Rendered review did not settle: ${JSON.stringify(ui)}`);
    }
    const result = await getPairs();
    const tree = await waitFor('duplicate sidebar count', getSidebar, (value) => (
      sidebarNode(value, 'system:duplicates')?.count === 0
    ), timeoutMs);
    if (result.total !== 0) throw new Error(`Resolved pair remains detected: ${JSON.stringify(result)}`);
    const sidebarText = await cdp.evaluate(buildDomTextContainerExpression('Duplicates'));
    return {
      action: 'keep_both',
      rendered_empty_state: ui.reviewComplete.found ? 'Review complete' : 'No duplicate pairs',
      rendered_sidebar: sidebarText.text,
      duplicate_count: sidebarNode(tree, 'system:duplicates')?.count,
      remaining_detected: result.total,
      control: control.text,
    };
  });

  return { steps, scan, library_root: libraryRoot };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) return usage();
  const platform = normalizePlatform(args.platform || process.platform);
  const timeoutMs = Number(args.timeout || DEFAULT_TIMEOUT_MS);
  const reportPath = path.resolve(args.report || 'artifacts/duplicates/smoke.json');
  const temporaryRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-duplicate-smoke-'));
  const appData = path.join(temporaryRoot, 'app-data');
  const libraryRoot = path.join(temporaryRoot, 'smoke.library');
  const sourceRoot = path.join(temporaryRoot, 'fixtures');
  await fs.mkdir(sourceRoot, { recursive: true });
  await Promise.all([
    fs.writeFile(path.join(sourceRoot, 'active-a.png'), createPng({ marker: 'a' })),
    fs.writeFile(path.join(sourceRoot, 'active-b.png'), createPng({ marker: 'b' })),
    fs.writeFile(path.join(sourceRoot, 'inbox.png'), createPng({ variant: 'different', marker: 'inbox' })),
  ]);

  const startedAt = new Date().toISOString();
  const report = {
    kind: 'electron-duplicate-review-smoke',
    platform,
    started_at: startedAt,
    finished_at: null,
    passed: false,
    steps: [],
    gaps: [
      'This harness asserts accessible rendered controls and state through CDP DOM evaluation, not pixel-level visual fidelity.',
    'Collection ownership conflict resolution and loser blob reclamation are covered by backend-focused fixtures, not this rendered smoke.',
    ],
    executable: null,
    scan_evidence: null,
    restart_evidence: null,
    stdout_tail: '',
    stderr_tail: '',
  };

  let executable = null;
  let first = null;
  let second = null;
  try {
    executable = args.executable
      ? path.resolve(args.executable)
      : await findUnpackedExecutable({ distDir: path.resolve(args.dist || 'dist'), platform });
    report.executable = executable;
    const firstPort = 9400 + Math.floor(Math.random() * 500);
    first = await launch(executable, { appData, libraryRoot, port: firstPort, timeoutMs });
    const scenario = await runScenario({ cdp: first.cdp, sourceRoot, libraryRoot, timeoutMs });
    report.steps = scenario.steps;
    report.scan_evidence = scenario.scan;
    await first.cdp.close();
    await stopProcess(first.child);
    first = null;

    const secondPort = firstPort + 1;
    second = await launch(executable, { appData, libraryRoot, port: secondPort, timeoutMs });
    await waitFor(
      'Duplicates navigation after restart',
      () => second.cdp.evaluate(readTextExpression('Duplicates')),
      (value) => value.found,
      timeoutMs,
    );
    const navigation = await second.cdp.evaluate(clickTextExpression('Duplicates'));
    if (!navigation.found) throw new Error('Duplicates navigation item was not clickable after restart');
    const restartUi = () => second.cdp.evaluate(`({
      noPairs: ${readRoleExpression('No duplicate pairs')},
      reviewComplete: ${readRoleExpression('Review complete')},
    })`);
    const rendered = await waitFor(
      'rendered duplicate empty state after restart',
      restartUi,
      (value) => value.noPairs.found || value.reviewComplete.found,
      timeoutMs,
    );
    const restart = await second.cdp.evaluate(invokeExpression('get_sidebar_tree'));
    const pairs = await second.cdp.evaluate(invokeExpression('get_duplicate_pairs', { limit: 20, status: 'detected' }));
    const allCount = sidebarNode(restart, 'system:active')?.count;
    const inboxCount = sidebarNode(restart, 'system:inbox')?.count;
    const duplicateCount = sidebarNode(restart, 'system:duplicates')?.count;
    if (allCount !== 2 || inboxCount !== 1 || duplicateCount !== 0 || pairs.total !== 0) {
      throw new Error(`Restart state mismatch: ${JSON.stringify({ allCount, inboxCount, duplicateCount, detected: pairs.total })}`);
    }
    report.restart_evidence = {
      rendered_empty_state: rendered.reviewComplete.found ? 'Review complete' : 'No duplicate pairs',
      all_count: allCount,
      inbox_count: inboxCount,
      duplicate_count: duplicateCount,
      detected_pairs: pairs.total,
    };
    report.steps.push({
      name: 'restart and verify persisted state',
      passed: true,
      details: report.restart_evidence,
    });
    report.passed = true;
  } catch (error) {
    if (error?.smokeSteps) report.steps = error.smokeSteps;
    if (error?.launchOutput) Object.assign(report, error.launchOutput);
    if (error?.launchChild) await stopProcess(error.launchChild);
    report.error = error instanceof Error ? error.message : String(error);
  } finally {
    if (first) {
      first.cdp.close();
      await stopProcess(first.child);
      Object.assign(report, first.getOutput());
    }
    if (second) {
      second.cdp.close();
      await stopProcess(second.child);
      Object.assign(report, second.getOutput());
    }
    report.finished_at = new Date().toISOString();
    await fs.mkdir(path.dirname(reportPath), { recursive: true });
    await fs.writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
    await fs.rm(temporaryRoot, { recursive: true, force: true });
  }

  console.log(`[duplicate-smoke] ${report.passed ? 'PASS' : 'FAIL'} ${reportPath}`);
  if (!report.passed) process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  void main();
}

export {
  buildDomExpression,
  buildDomTextContainerExpression,
  createPng,
  parseArgs,
  sidebarNode,
};
