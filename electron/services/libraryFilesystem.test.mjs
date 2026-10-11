import { expect, test } from 'vitest';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { isFailedCloudJoinDirectory, isInsideLibraryPackage } from './libraryFilesystem.mjs';

test('recognizes locations nested anywhere inside a library package', () => {
  expect(isInsideLibraryPackage(path.posix, '/Pictures/Main.library')).toBe(true);
  expect(isInsideLibraryPackage(path.posix, '/Pictures/Main.library/exports')).toBe(true);
  expect(isInsideLibraryPackage(path.posix, '/Pictures/Libraries')).toBe(false);
  expect(isInsideLibraryPackage(path.win32, 'D:\\Main.library\\exports')).toBe(true);
  expect(isInsideLibraryPackage(path.win32, 'D:\\Libraries')).toBe(false);
});

test('recognizes only disposable remnants from an interrupted cloud join', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-failed-cloud-join-'));
  try {
    await fs.mkdir(path.join(root, 'blobs'));
    await fs.writeFile(path.join(root, 'failed-cloud-join-1234.sqlite'), 'restored database');
    await expect(isFailedCloudJoinDirectory(fs, root)).resolves.toBe(true);

    await fs.writeFile(path.join(root, 'library.sqlite'), 'active database');
    await expect(isFailedCloudJoinDirectory(fs, root)).resolves.toBe(false);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});
