#!/usr/bin/env node
import { promises as fs } from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();
const TARGET_DIRS = ['src', 'core/src', 'electron', 'native/picto-node/src'];
const EXTENSIONS = new Set(['.ts', '.tsx', '.rs', '.mjs', '.cjs']);

const DEFAULT_LIMITS = {
  '.ts': 700,
  '.tsx': 700,
  '.rs': 900,
  '.mjs': 600,
  '.cjs': 600,
};

// Transitional overrides for known hotspots. These are explicit phase-1
// exceptions tracked under PBI-171 and should be tightened in phase-2.
// Keep this list small and intentional; do not add files without a reason.
const EXCEPTION_BUDGETS = {
  'core/src/sqlite/schema.rs': { limit: 1600, phase: 'P1', reason: 'split schema fragments by domain' },
  'core/src/gallery_dl_runner.rs': { limit: 1520, phase: 'P1', reason: 'extract per-site modules' },
  'core/src/sqlite/files.rs': { limit: 1500, phase: 'P1', reason: 'split queries/lifecycle helpers' },
  'core/src/sqlite/compilers.rs': { limit: 1300, phase: 'P1', reason: 'split compiler stages' },
  'core/src/sqlite/collections.rs': { limit: 1350, phase: 'P1', reason: 'extract collection query groups' },
  'core/src/files/mod.rs': { limit: 1100, phase: 'P1', reason: 'split file processors by mime family' },
  'core/src/subscription_sync.rs': { limit: 1150, phase: 'P1', reason: 'extract import and metadata merge paths' },
  'core/src/grid_controller.rs': { limit: 1000, phase: 'P1', reason: 'extract status-specific branches' },
  'core/src/sqlite/folders.rs': { limit: 920, phase: 'P1', reason: 'split folder CRUD and ordering helpers' },
  'core/src/sqlite/tags.rs': { limit: 1300, phase: 'P1', reason: 'legacy hotspot' },
  'core/src/ptr_sync.rs': { limit: 1300, phase: 'P1', reason: 'legacy hotspot' },
  'core/src/ptr_controller.rs': { limit: 1100, phase: 'P1', reason: 'legacy hotspot' },
  'core/src/files/specialty.rs': { limit: 1000, phase: 'P1', reason: 'legacy hotspot' },
  'core/src/sqlite_ptr/bootstrap.rs': { limit: 1800, phase: 'P1', reason: 'legacy hotspot' },
  'core/src/sqlite_ptr/sync.rs': { limit: 1300, phase: 'P1', reason: 'legacy hotspot' },
  'src/features/sidebar/components/FolderTree.tsx': { limit: 1000, phase: 'P1', reason: 'extract folder action registry hooks' },
  'src/features/grid/hooks/useGridContextMenu.tsx': { limit: 950, phase: 'P1', reason: 'split action groups by domain' },
  'src/platform/api.ts': { limit: 850, phase: 'P1', reason: 'split invoke surface by domain' },
  'src/features/tags/components/TagManager.tsx': { limit: 950, phase: 'P1', reason: 'extract table/filters subcomponents' },
  'src/features/grid/DetailWindow.tsx': { limit: 750, phase: 'P1', reason: 'shared viewer core extraction' },
  'src/features/grid/imageAtlas.ts': { limit: 900, phase: 'P1', reason: 'legacy hotspot' },
  'src/features/tags/components/TagSelectPanel.tsx': { limit: 900, phase: 'P1', reason: 'legacy hotspot' },
  'src/features/grid/ImageGrid.tsx': { limit: 2400, phase: 'P1', reason: 'legacy hotspot' },
  'src/features/grid/CanvasGrid.tsx': { limit: 2100, phase: 'P1', reason: 'legacy hotspot' },
  'src/features/grid/VirtualGrid.tsx': { limit: 1200, phase: 'P1', reason: 'legacy hotspot' },
  'electron/main.mjs': { limit: 1600, phase: 'P1', reason: 'legacy hotspot' },
};

async function walk(dir, out) {
  let entries;
  try {
    entries = await fs.readdir(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    if (entry.name.startsWith('.')) continue;
    const absolute = path.join(dir, entry.name);
    const rel = path.relative(ROOT, absolute).replaceAll(path.sep, '/');
    if (entry.isDirectory()) {
      if (entry.name === 'target' || entry.name === 'node_modules' || entry.name === 'dist') continue;
      await walk(absolute, out);
      continue;
    }
    const ext = path.extname(entry.name);
    if (!EXTENSIONS.has(ext)) continue;
    out.push(rel);
  }
}

function countLines(text) {
  if (text.length === 0) return 0;
  return text.split(/\r\n|\n|\r/).length;
}

async function main() {
  const files = [];
  for (const dir of TARGET_DIRS) {
    await walk(path.join(ROOT, dir), files);
  }

  const stats = [];
  for (const rel of files) {
    const content = await fs.readFile(path.join(ROOT, rel), 'utf8');
    const ext = path.extname(rel);
    const lines = countLines(content);
    const exception = EXCEPTION_BUDGETS[rel] ?? null;
    const defaultLimit = DEFAULT_LIMITS[ext] ?? 800;
    const limit = exception?.limit ?? defaultLimit;
    stats.push({
      rel,
      lines,
      limit,
      defaultLimit,
      hasException: !!exception,
      exceptionPhase: exception?.phase ?? null,
      over: lines > limit,
    });
  }

  stats.sort((a, b) => b.lines - a.lines);
  const top = stats.slice(0, 25);

  console.log('Top file lengths (lines):');
  for (const s of top) {
    const marker = s.over ? ' !' : '  ';
    const phaseNote = s.hasException ? `, exception ${s.exceptionPhase}` : '';
    console.log(`${marker} ${String(s.lines).padStart(5)}  ${s.rel} (limit ${s.limit}${phaseNote})`);
  }

  const offenders = stats.filter((s) => s.over);
  const newOverDefaultWithoutException = stats.filter(
    (s) => !s.hasException && s.lines > s.defaultLimit,
  );
  if (offenders.length > 0) {
    console.error(`\nFile-length budget violations: ${offenders.length}`);
    for (const off of offenders.slice(0, 50)) {
      console.error(` - ${off.rel}: ${off.lines} > ${off.limit}`);
    }
    if (newOverDefaultWithoutException.length > 0) {
      console.error('\nNew over-budget files without explicit exception entries:');
      for (const file of newOverDefaultWithoutException.slice(0, 50)) {
        console.error(` - ${file.rel}: ${file.lines} > default ${file.defaultLimit}`);
      }
      console.error(
        '\nAdd an explicit phase exception entry in EXCEPTION_BUDGETS with reason + follow-up split plan.',
      );
    }
    process.exit(1);
  }

  console.log('\nAll file-length budgets passed.');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
