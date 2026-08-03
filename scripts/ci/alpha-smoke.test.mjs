import { describe, expect, it } from 'vitest';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {
  createSmokeReportParser,
  evaluateRun,
  findUnpackedExecutable,
  removeTemporaryRoot,
} from './alpha-smoke.mjs';

describe('packaged smoke runner', () => {
  it('finds the unpacked product executable for each package layout', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-alpha-smoke-test-'));
    try {
      const linux = path.join(root, 'linux-unpacked', 'Picto');
      const windows = path.join(root, 'win-unpacked', 'Picto.exe');
      const mac = path.join(root, 'mac-arm64', 'Picto.app', 'Contents', 'MacOS', 'Picto');
      await Promise.all([linux, windows, mac].map(async (file) => {
        await fs.mkdir(path.dirname(file), { recursive: true });
        await fs.writeFile(file, 'fixture');
      }));

      await expect(findUnpackedExecutable({ distDir: root, platform: 'linux' })).resolves.toBe(linux);
      await expect(findUnpackedExecutable({ distDir: root, platform: 'win32' })).resolves.toBe(windows);
      await expect(findUnpackedExecutable({ distDir: root, platform: 'darwin' })).resolves.toBe(mac);
    } finally {
      await fs.rm(root, { recursive: true, force: true });
    }
  });

  it('chooses the newest unpacked executable with a deterministic path tie-break', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-alpha-smoke-test-'));
    try {
      const older = path.join(root, 'older', 'linux-unpacked', 'Picto');
      const newerA = path.join(root, 'newer-a', 'linux-unpacked', 'Picto');
      const newerB = path.join(root, 'newer-b', 'linux-unpacked', 'Picto');
      await Promise.all([older, newerA, newerB].map(async (file) => {
        await fs.mkdir(path.dirname(file), { recursive: true });
        await fs.writeFile(file, 'fixture');
      }));
      const oldTime = new Date('2025-01-01T00:00:00Z');
      const newTime = new Date('2026-01-01T00:00:00Z');
      await fs.utimes(older, oldTime, oldTime);
      await fs.utimes(newerA, newTime, newTime);
      await fs.utimes(newerB, newTime, newTime);

      await expect(findUnpackedExecutable({ distDir: root, platform: 'linux' })).resolves.toBe(newerA);
    } finally {
      await fs.rm(root, { recursive: true, force: true });
    }
  });

  it('parses chunked smoke reports and retains malformed reports', () => {
    const parser = createSmokeReportParser();
    parser.push('noise\n[picto-packaged-smoke] {"event":"native-library-');
    parser.push('initialized"}\n[picto-packaged-smoke] not-json\n');
    const result = parser.finish();

    expect(result.reports).toEqual([{ event: 'native-library-initialized' }]);
    expect(result.malformed).toHaveLength(1);
  });

  it('requires clean native shutdown before passing', () => {
    const baseRun = {
      code: 0,
      signal: null,
      timedOut: false,
      spawnError: null,
      malformed: [],
      reports: [
        { event: 'native-library-initialized' },
        { event: 'did-finish-load' },
        { event: 'settle-complete' },
      ],
    };

    expect(evaluateRun(baseRun)).toContain('missing smoke events: native-library-closed');
    expect(evaluateRun({
      ...baseRun,
      reports: [...baseRun.reports, { event: 'native-library-closed' }],
    })).toEqual([]);
  });

  it('uses retry-aware removal and reports cleanup failure truthfully', async () => {
    let options;
    const result = await removeTemporaryRoot('/tmp/picto-alpha-smoke-test', async (_root, receivedOptions) => {
      options = receivedOptions;
      throw new Error('locked');
    });

    expect(options).toMatchObject({ recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
    expect(result).toEqual({ succeeded: false, error: 'locked' });
  });
});
