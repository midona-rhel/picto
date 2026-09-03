import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC_ROOT = join(process.cwd(), 'src');
const OWNER = 'runtime/shortcutRuntime.ts';

function productionSources(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return productionSources(path);
    if (!/\.tsx?$/.test(entry.name) || /\.test\.tsx?$/.test(entry.name)) return [];
    return [path];
  });
}

describe('renderer shortcut ownership', () => {
  it('keeps the only global keydown listener in the shortcut runtime', () => {
    const offenders = productionSources(SRC_ROOT)
      .filter((path) => relative(SRC_ROOT, path).replace(/\\/g, '/') !== OWNER)
      .filter((path) => /window\.addEventListener\(['"]keydown/.test(readFileSync(path, 'utf8')))
      .map((path) => relative(SRC_ROOT, path));

    expect(offenders).toEqual([]);
  });

  it('resolves standard tooltip shortcuts from registry IDs', () => {
    const offenders = productionSources(SRC_ROOT)
      .filter((path) => /<KbdTooltip[^>]*\bshortcut=["']/.test(readFileSync(path, 'utf8')))
      .map((path) => relative(SRC_ROOT, path));

    expect(offenders).toEqual([]);
  });

  it('does not use native browser tooltips on action buttons', () => {
    const offenders = productionSources(SRC_ROOT)
      .filter((path) => /<button[^>]*\btitle=/s.test(readFileSync(path, 'utf8')))
      .map((path) => relative(SRC_ROOT, path));

    expect(offenders).toEqual([]);
  });
});
