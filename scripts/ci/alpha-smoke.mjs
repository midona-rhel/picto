#!/usr/bin/env node

import { spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import {
  evaluateResult as evaluateGalleryDlBridge,
  runSidecar as runGalleryDlBridge,
} from './gallery-dl-bridge-smoke.mjs';

const SMOKE_PREFIX = '[picto-packaged-smoke] ';
const REQUIRED_EVENTS = new Set([
  'native-library-initialized',
  'did-finish-load',
  'settle-complete',
  'native-library-closed',
]);
const FAILURE_EVENTS = new Set([
  'did-fail-load',
  'preload-error',
  'render-process-gone',
  'window-error',
  'uncaught-exception',
  'unhandled-rejection',
  'bootstrap-failed',
  'packaged-smoke-failed',
  'shutdown-failed',
]);
const PROCESS_TIMEOUT_MS = 30_000;
const OUTPUT_LIMIT = 64 * 1024;

export function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index];
    if (key === '--platform' || key === '--report' || key === '--dist') {
      args[key.slice(2)] = argv[++index];
    }
  }
  return args;
}

export function normalizePlatform(platform) {
  if (platform === 'darwin' || platform === 'mac' || platform === 'macos') return 'darwin';
  if (platform === 'win32' || platform === 'win' || platform === 'windows') return 'win32';
  if (platform === 'linux') return 'linux';
  throw new Error(`Unsupported platform '${platform}'. Use darwin, linux, or win32.`);
}

async function collectFiles(root) {
  const files = [];
  const directories = [root];
  while (directories.length > 0) {
    const directory = directories.pop();
    const entries = await fs.readdir(directory, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = path.join(directory, entry.name);
      if (entry.isDirectory()) directories.push(fullPath);
      else if (entry.isFile()) files.push(fullPath);
    }
  }
  return files;
}

function hasUnpackedParent(file, platform) {
  const parts = file.split(path.sep).map((part) => part.toLowerCase());
  if (platform === 'darwin') return parts.some((part) => part.endsWith('.app'));
  return parts.includes(`${platform === 'win32' ? 'win' : 'linux'}-unpacked`);
}

export async function findUnpackedExecutable({ distDir, platform, productName = 'Picto' }) {
  const normalizedPlatform = normalizePlatform(platform);
  const files = await collectFiles(distDir);
  const expectedNames = normalizedPlatform === 'win32'
    ? new Set([`${productName}.exe`.toLowerCase()])
    : new Set([productName.toLowerCase()]);
  const candidates = files
    .filter((file) => expectedNames.has(path.basename(file).toLowerCase()))
    .filter((file) => hasUnpackedParent(file, normalizedPlatform));

  if (candidates.length === 0) {
    throw new Error(`Expected an unpacked ${productName} executable in ${distDir}; found none.`);
  }

  const ranked = await Promise.all(candidates.map(async (file) => ({
    file,
    mtimeMs: (await fs.stat(file)).mtimeMs,
  })));
  ranked.sort((left, right) => {
    if (left.mtimeMs !== right.mtimeMs) return right.mtimeMs - left.mtimeMs;
    return left.file < right.file ? -1 : left.file > right.file ? 1 : 0;
  });
  return ranked[0].file;
}

export function findPackagedGalleryDlBridge(executable, platform) {
  const normalizedPlatform = normalizePlatform(platform);
  const executableDir = path.dirname(executable);
  const resourcesRoot = normalizedPlatform === 'darwin'
    ? path.dirname(executableDir)
    : executableDir;
  const binaryName = normalizedPlatform === 'win32'
    ? 'picto-gallery-dl-bridge.exe'
    : 'picto-gallery-dl-bridge';
  return path.join(resourcesRoot, 'gallery-dl', binaryName);
}

export function findPackagedOnlyFansBridge(executable, platform) {
  const normalizedPlatform = normalizePlatform(platform);
  const executableDir = path.dirname(executable);
  const resourcesRoot = normalizedPlatform === 'darwin'
    ? path.dirname(executableDir)
    : executableDir;
  const binaryName = normalizedPlatform === 'win32'
    ? 'picto-onlyfans-bridge.exe'
    : 'picto-onlyfans-bridge';
  return path.join(resourcesRoot, 'onlyfans', binaryName);
}

function evaluateOnlyFansBridge(run) {
  const reasons = [];
  if (run.spawnError) reasons.push(`launch failed: ${run.spawnError}`);
  if (run.timedOut) reasons.push('process timed out');
  if (run.code !== 0) reasons.push(`expected exit code 0, received ${run.code ?? run.signal ?? 'unknown'}`);
  if (!run.events.some((event) => event.event === 'onlyfans_self_test' && event.ofscraper_imported === true)) {
    reasons.push('missing OF-Scraper import proof');
  }
  if (run.malformed.length > 0) reasons.push('malformed sidecar output');
  return reasons;
}

export function createSmokeReportParser() {
  let pending = '';
  const reports = [];
  const malformed = [];

  const parseLine = (line) => {
    if (!line.startsWith(SMOKE_PREFIX)) return;
    try {
      const report = JSON.parse(line.slice(SMOKE_PREFIX.length));
      if (!report || typeof report.event !== 'string') throw new Error('missing event');
      reports.push(report);
    } catch {
      malformed.push(line);
    }
  };

  return {
    push(chunk) {
      pending += chunk;
      const lines = pending.split(/\r?\n/);
      pending = lines.pop();
      for (const line of lines) parseLine(line);
    },
    finish() {
      if (pending) parseLine(pending);
      pending = '';
      return { reports, malformed };
    },
  };
}

function appendOutput(current, chunk) {
  const next = current + chunk;
  return next.length > OUTPUT_LIMIT ? next.slice(-OUTPUT_LIMIT) : next;
}

function launch(executable, env) {
  return new Promise((resolve) => {
    const child = spawn(executable, [], { cwd: path.dirname(executable), env, windowsHide: true });
    const parser = createSmokeReportParser();
    let stdout = '';
    let stderr = '';
    let timedOut = false;
    let spawnError = null;
    const timeout = setTimeout(() => {
      timedOut = true;
      child.kill();
    }, PROCESS_TIMEOUT_MS);

    child.stdout.on('data', (chunk) => {
      const text = chunk.toString();
      stdout = appendOutput(stdout, text);
      parser.push(text);
    });
    child.stderr.on('data', (chunk) => {
      stderr = appendOutput(stderr, chunk.toString());
    });
    child.on('error', (error) => {
      spawnError = error.message;
    });
    child.on('close', (code, signal) => {
      clearTimeout(timeout);
      const parsed = parser.finish();
      resolve({ code, signal, timedOut, spawnError, stdout, stderr, ...parsed });
    });
  });
}

export function evaluateRun(run, requiredEvents = REQUIRED_EVENTS) {
  const events = new Set(run.reports.map((report) => report.event));
  const failures = run.reports.filter((report) => FAILURE_EVENTS.has(report.event));
  const missing = [...requiredEvents].filter((event) => !events.has(event));
  const reasons = [];
  if (run.spawnError) reasons.push(`launch failed: ${run.spawnError}`);
  if (run.timedOut) reasons.push('process timed out');
  if (run.code !== 0) reasons.push(`expected exit code 0, received ${run.code ?? run.signal ?? 'unknown'}`);
  if (missing.length > 0) reasons.push(`missing smoke events: ${missing.join(', ')}`);
  if (failures.length > 0) reasons.push(`reported failures: ${failures.map((report) => report.event).join(', ')}`);
  if (run.malformed.length > 0) reasons.push('malformed smoke report output');
  return reasons;
}

export async function removeTemporaryRoot(temporaryRoot, remove = fs.rm) {
  try {
    await remove(temporaryRoot, {
      recursive: true,
      force: true,
      maxRetries: 3,
      retryDelay: 100,
    });
    return { succeeded: true, error: null };
  } catch (error) {
    return {
      succeeded: false,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const platform = normalizePlatform(args.platform || process.platform);
  const distDir = path.resolve(args.dist || 'dist');
  const reportPath = path.resolve(args.report || path.join('artifacts', 'alpha-smoke', `${platform}.json`));
  const startedAt = new Date().toISOString();
  let temporaryRoot = null;
  const runs = [];
  let temporaryRootCreated = false;
  let cleanupSucceeded = true;
  let executable = null;
  let galleryDlBridge = null;
  let galleryDlBridgeRun = null;
  let onlyFansBridge = null;
  let onlyFansBridgeRun = null;
  let setupError = null;

  try {
    executable = await findUnpackedExecutable({ distDir, platform });
    galleryDlBridge = findPackagedGalleryDlBridge(executable, platform);
    galleryDlBridgeRun = await runGalleryDlBridge(galleryDlBridge, 30_000);
    const galleryDlFailures = evaluateGalleryDlBridge(galleryDlBridgeRun);
    if (galleryDlFailures.length > 0) {
      throw new Error(`packaged gallery-dl bridge failed: ${galleryDlFailures.join('; ')}`);
    }
    onlyFansBridge = findPackagedOnlyFansBridge(executable, platform);
    onlyFansBridgeRun = await runGalleryDlBridge(onlyFansBridge, 30_000);
    const onlyFansFailures = evaluateOnlyFansBridge(onlyFansBridgeRun);
    if (onlyFansFailures.length > 0) {
      throw new Error(`packaged OnlyFans bridge failed: ${onlyFansFailures.join('; ')}`);
    }
    temporaryRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-packaged-smoke-'));
    temporaryRootCreated = true;
    const home = path.join(temporaryRoot, 'home');
    const appData = path.join(temporaryRoot, 'app-data');
    const library = path.join(temporaryRoot, 'smoke.library');
    await Promise.all([
      fs.mkdir(home),
      fs.mkdir(appData),
      fs.mkdir(library),
    ]);
    const env = {
      ...process.env,
      HOME: home,
      USERPROFILE: home,
      PICTO_PACKAGED_SMOKE: '1',
      PICTO_SMOKE_APP_DATA: appData,
      PICTO_LIBRARY_ROOT: library,
    };
    delete env.ELECTRON_RUN_AS_NODE;
    runs.push({ name: 'packaged-launch', ...await launch(executable, env) });
  } catch (error) {
    setupError = error instanceof Error ? error.message : String(error);
  } finally {
    if (temporaryRoot) {
      const cleanup = await removeTemporaryRoot(temporaryRoot);
      cleanupSucceeded = cleanup.succeeded;
      if (!cleanupSucceeded) setupError ??= `failed to remove temporary root: ${cleanup.error}`;
    }
  }

  const reasons = setupError ? [setupError] : runs.flatMap((run) =>
    evaluateRun(run).map((reason) => `${run.name}: ${reason}`),
  );
  if (!setupError && runs.length !== 1) reasons.push(`expected one packaged launch, completed ${runs.length}`);
  if (temporaryRootCreated && !cleanupSucceeded) reasons.push('temporary root was not removed');
  const report = {
    platform,
    executable,
    gallery_dl_bridge: galleryDlBridge,
    gallery_dl_bridge_passed: galleryDlBridgeRun
      ? evaluateGalleryDlBridge(galleryDlBridgeRun).length === 0
      : false,
    onlyfans_bridge: onlyFansBridge,
    onlyfans_bridge_passed: onlyFansBridgeRun
      ? evaluateOnlyFansBridge(onlyFansBridgeRun).length === 0
      : false,
    started_at: startedAt,
    finished_at: new Date().toISOString(),
    passed: reasons.length === 0,
    reasons,
    temporary_root_removed: temporaryRootCreated ? cleanupSucceeded : null,
    exit_code: runs.find((run) => run.code !== 0)?.code ?? runs.at(-1)?.code ?? null,
    signal: runs.find((run) => run.signal)?.signal ?? null,
    timed_out: runs.some((run) => run.timedOut),
    events: runs.flatMap((run) => run.reports),
    stdout_tail: runs.map((run) => `[${run.name}]\n${run.stdout}`).join('\n'),
    stderr_tail: runs.map((run) => `[${run.name}]\n${run.stderr}`).join('\n'),
    runs,
  };
  await fs.mkdir(path.dirname(reportPath), { recursive: true });
  await fs.writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(`[alpha-smoke] ${report.passed ? 'PASS' : 'FAIL'} ${reportPath}`);
  if (!report.passed) {
    for (const reason of reasons) console.error(`[alpha-smoke] ${reason}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  void main();
}
