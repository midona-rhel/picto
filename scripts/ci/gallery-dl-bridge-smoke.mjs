#!/usr/bin/env node

import { spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const DEFAULT_TIMEOUT_MS = 10_000;
const OUTPUT_LIMIT = 64 * 1024;
const SELF_TEST_EVENT = 'bridge_self_test';

function usage() {
  return `Usage: node scripts/ci/gallery-dl-bridge-smoke.mjs <executable> [options]

Launches a packaged gallery-dl sidecar without network access or credentials and
checks its local self-test protocol.

Arguments:
  <executable>         path to the packaged gallery-dl bridge executable

Options:
  --executable <path>  equivalent to the positional executable argument
  --timeout <ms>       fail if the sidecar does not exit in time (default: ${DEFAULT_TIMEOUT_MS})
  -h, --help           show this help

The sidecar must accept --self-test and emit this JSON event on stdout:
  {"event":"${SELF_TEST_EVENT}","gallery_dl_imported":true,"rule34_adapter_initialized":true,"deviantart_adapter_initialized":true,"tumblr_adapter_initialized":true}
`;
}

export function parseArgs(argv) {
  let executable = null;
  let timeoutMs = DEFAULT_TIMEOUT_MS;
  let help = false;
  const positional = [];

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '-h' || argument === '--help') {
      help = true;
      continue;
    }
    if (argument === '--') {
      positional.push(...argv.slice(index + 1));
      break;
    }
    if (argument === '--executable') {
      executable = argv[++index];
      if (!executable) throw new Error('--executable requires a path');
      continue;
    }
    if (argument === '--timeout') {
      const value = argv[++index];
      if (!value) throw new Error('--timeout requires a positive integer');
      timeoutMs = Number(value);
      if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
        throw new Error(`Invalid timeout '${value}'; expected a positive integer in milliseconds`);
      }
      continue;
    }
    if (argument.startsWith('-')) {
      throw new Error(`Unknown option '${argument}'`);
    }
    positional.push(argument);
  }

  if (executable && positional.length > 0) {
    throw new Error('Provide the executable either positionally or with --executable, not both');
  }
  executable ??= positional[0] ?? null;
  if (positional.length > 1) throw new Error('Only one executable path may be provided');

  return { executable, timeoutMs, help };
}

export function parseNdjson(text) {
  const events = [];
  const malformed = [];
  const lines = String(text).split(/\r?\n/);

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const value = JSON.parse(trimmed);
      if (value && typeof value === 'object' && !Array.isArray(value)) events.push(value);
    } catch {
      // Sidecars may write human-readable diagnostics alongside NDJSON. Only
      // report malformed JSON-looking lines; ordinary logs are not protocol data.
      if (trimmed.startsWith('{') || trimmed.startsWith('[')) malformed.push(trimmed);
    }
  }

  return { events, malformed };
}

export function isValidSelfTestEvent(event) {
  return event?.event === SELF_TEST_EVENT
    && event.gallery_dl_imported === true
    && event.rule34_adapter_initialized === true
    && event.deviantart_adapter_initialized === true
    && event.tumblr_adapter_initialized === true;
}

function appendOutput(current, chunk) {
  const next = current + chunk;
  return next.length > OUTPUT_LIMIT ? next.slice(-OUTPUT_LIMIT) : next;
}

function terminate(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  child.kill();
}

export function evaluateResult(result) {
  const reasons = [];
  if (result.spawnError) reasons.push(`launch failed: ${result.spawnError}`);
  if (result.timedOut) reasons.push(`process timed out after ${result.timeoutMs} ms`);
  if (result.code !== 0) {
    const exit = result.code === null ? `signal ${result.signal ?? 'unknown'}` : `code ${result.code}`;
    reasons.push(`expected exit 0, received ${exit}`);
  }

  const selfTestEvents = result.events.filter(isValidSelfTestEvent);
  if (selfTestEvents.length === 0) {
    reasons.push(
      `missing valid ${SELF_TEST_EVENT} event proving gallery-dl import and source adapter initialization`,
    );
  }
  if (result.malformed.length > 0) {
    reasons.push(`malformed JSON/NDJSON output (${result.malformed.length} line${result.malformed.length === 1 ? '' : 's'})`);
  }
  return reasons;
}

export function runSidecar(executable, timeoutMs = DEFAULT_TIMEOUT_MS) {
  return new Promise((resolve) => {
    const child = spawn(executable, ['--self-test'], {
      cwd: path.dirname(executable),
      env: {
        ...process.env,
        PICTO_BRIDGE_SELF_TEST: '1',
        PICTO_BRIDGE_NO_NETWORK: '1',
        PICTO_BRIDGE_NO_CREDENTIALS: '1',
      },
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
      shell: false,
    });

    let stdout = '';
    let stderr = '';
    let timedOut = false;
    let spawnError = null;
    let settled = false;
    let forceKillTimer = null;

    const finish = (code, signal) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      if (forceKillTimer) clearTimeout(forceKillTimer);
      const parsed = parseNdjson(stdout);
      resolve({
        code,
        signal,
        timedOut,
        spawnError,
        timeoutMs,
        stdout,
        stderr,
        ...parsed,
      });
    };

    const timeout = setTimeout(() => {
      timedOut = true;
      terminate(child);
      forceKillTimer = setTimeout(() => {
        if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
      }, 500);
    }, timeoutMs);

    child.stdout.on('data', (chunk) => {
      stdout = appendOutput(stdout, chunk.toString());
    });
    child.stderr.on('data', (chunk) => {
      stderr = appendOutput(stderr, chunk.toString());
    });
    child.once('error', (error) => {
      spawnError = error.message;
    });
    child.once('close', finish);
  });
}

function formatOutput(label, output) {
  const trimmed = output.trim();
  return trimmed ? `\n${label} (last ${OUTPUT_LIMIT} bytes):\n${trimmed}` : '';
}

async function main(argv = process.argv.slice(2)) {
  let args;
  try {
    args = parseArgs(argv);
  } catch (error) {
    console.error(`[gallery-dl-bridge-smoke] ${error.message}\n\n${usage()}`);
    return 2;
  }

  if (args.help) {
    console.log(usage());
    return 0;
  }
  if (!args.executable) {
    console.error(`[gallery-dl-bridge-smoke] Missing executable path.\n\n${usage()}`);
    return 2;
  }

  const executable = path.resolve(args.executable);
  try {
    const stat = await fs.stat(executable);
    if (!stat.isFile()) throw new Error('path is not a file');
  } catch (error) {
    console.error(`[gallery-dl-bridge-smoke] Cannot use executable '${executable}': ${error.message}`);
    return 2;
  }

  const result = await runSidecar(executable, args.timeoutMs);
  const failures = evaluateResult(result);
  if (failures.length > 0) {
    console.error(`[gallery-dl-bridge-smoke] FAILED\n- ${failures.join('\n- ')}`);
    console.error(formatOutput('stdout', result.stdout));
    console.error(formatOutput('stderr', result.stderr));
    return 1;
  }

  console.log(`[gallery-dl-bridge-smoke] PASS: gallery-dl imported and source adapters initialized`);
  return 0;
}

const invokedPath = process.argv[1] ? pathToFileURL(path.resolve(process.argv[1])).href : null;
if (invokedPath === import.meta.url) {
  main().then((code) => {
    process.exitCode = code;
  }).catch((error) => {
    console.error(`[gallery-dl-bridge-smoke] Unexpected verifier failure: ${error.stack ?? error}`);
    process.exitCode = 1;
  });
}
