#!/usr/bin/env node

import { spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';

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
  'sync-smoke-failed',
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
  let setupError = null;

  try {
    executable = await findUnpackedExecutable({ distDir, platform });
    temporaryRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-packaged-smoke-'));
    temporaryRootCreated = true;
    const deviceAHome = path.join(temporaryRoot, 'device-a-home');
    const deviceBHome = path.join(temporaryRoot, 'device-b-home');
    const deviceAAppData = path.join(temporaryRoot, 'device-a-app-data');
    const deviceBAppData = path.join(temporaryRoot, 'device-b-app-data');
    const deviceALibrary = path.join(temporaryRoot, 'device-a-library');
    const deviceBLibrary = path.join(temporaryRoot, 'device-b-library');
    const syncRoot = path.join(temporaryRoot, 'sync-root');
    const mediaPath = path.join(temporaryRoot, 'smoke.bmp');
    const mediaBytes = Buffer.from(
      '424d3a0000000000000036000000280000000100000001000000010018000000000004000000130b0000130b000000000000000000000000ff00',
      'hex',
    );
    const mediaHash = createHash('sha256').update(mediaBytes).digest('hex');
    await Promise.all([
      fs.mkdir(deviceAHome),
      fs.mkdir(deviceBHome),
      fs.mkdir(deviceAAppData),
      fs.mkdir(deviceBAppData),
      fs.mkdir(deviceALibrary),
      fs.mkdir(deviceBLibrary),
      fs.mkdir(syncRoot),
      fs.writeFile(mediaPath, mediaBytes),
    ]);
    const phases = [
      {
        name: 'device-a-publish',
        phase: 'publish',
        expected: 'sync-device-a-published',
        home: deviceAHome,
        appData: deviceAAppData,
        library: deviceALibrary,
      },
      {
        name: 'device-b-publish',
        phase: 'peer',
        expected: 'sync-device-b-published',
        home: deviceBHome,
        appData: deviceBAppData,
        library: deviceBLibrary,
      },
      {
        name: 'device-a-verify',
        phase: 'verify',
        expected: 'two-device-sync-complete',
        home: deviceAHome,
        appData: deviceAAppData,
        library: deviceALibrary,
      },
    ];

    for (const phase of phases) {
      const env = {
        ...process.env,
        HOME: phase.home,
        USERPROFILE: phase.home,
        PICTO_PACKAGED_SMOKE: '1',
        PICTO_SMOKE_APP_DATA: phase.appData,
        PICTO_LIBRARY_ROOT: phase.library,
        PICTO_SMOKE_SYNC_ROOT: syncRoot,
        PICTO_SMOKE_SYNC_PHASE: phase.phase,
        PICTO_SMOKE_MEDIA_PATH: mediaPath,
        PICTO_SMOKE_MEDIA_HASH: mediaHash,
      };
      delete env.ELECTRON_RUN_AS_NODE;
      const run = await launch(executable, env);
      runs.push({ name: phase.name, expected: phase.expected, ...run });
      const required = new Set([...REQUIRED_EVENTS, phase.expected]);
      if (evaluateRun(run, required).length > 0) break;
    }
  } catch (error) {
    setupError = error instanceof Error ? error.message : String(error);
  } finally {
    if (temporaryRoot) {
      const cleanup = await removeTemporaryRoot(temporaryRoot);
      cleanupSucceeded = cleanup.succeeded;
      if (!cleanupSucceeded) setupError ??= `failed to remove temporary root: ${cleanup.error}`;
    }
  }

  const reasons = setupError ? [setupError] : runs.flatMap((run) => {
    const required = new Set([...REQUIRED_EVENTS, run.expected]);
    return evaluateRun(run, required).map((reason) => `${run.name}: ${reason}`);
  });
  if (!setupError && runs.length !== 3) reasons.push(`expected 3 smoke phases, completed ${runs.length}`);
  if (!setupError && runs.length === 3) {
    const event = (name) => runs.flatMap((run) => run.reports).find((report) => report.event === name);
    const deviceA = event('sync-device-a-published')?.device_id;
    const deviceB = event('sync-device-b-published')?.device_id;
    const restartedA = event('two-device-sync-complete')?.device_id;
    if (!deviceA || !deviceB || !restartedA) reasons.push('sync smoke did not report every device identity');
    else if (deviceA === deviceB) reasons.push('device A and device B used the same sync identity');
    else if (deviceA !== restartedA) reasons.push('device A identity changed across restart');
  }
  if (temporaryRootCreated && !cleanupSucceeded) reasons.push('temporary root was not removed');
  const report = {
    platform,
    executable,
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
    runs: runs.map(({ expected: _expected, ...run }) => run),
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
