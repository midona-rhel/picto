import { build } from 'esbuild';
import { mkdir, readFile, writeFile, access } from 'node:fs/promises';
import path from 'node:path';

const root = process.cwd();
if (!process.argv[2] || !process.argv[3]) throw new Error('Expected frozen baseline directory and a new output JSON path');
const baselineRoot = path.resolve(process.argv[2]);
const outputPath = path.resolve(process.argv[3]);
try { await access(outputPath); throw new Error('Refusing to overwrite benchmark measurements'); }
catch (error) { if (error.code !== 'ENOENT') throw error; }
const baselineManifest = JSON.parse(await readFile(path.join(baselineRoot, 'source-manifest.json'), 'utf8'));
Object.defineProperty(globalThis, 'navigator', { value: { platform: 'Win32', userAgent: 'Windows' }, configurable: true });
const stats = values => {
  const sorted = [...values].sort((a, b) => a - b);
  return { n: values.length, samples_ms: values, p50: sorted[Math.ceil(values.length * .5) - 1],
    p95: sorted[Math.ceil(values.length * .95) - 1], max: sorted.at(-1),
    ...(values.length >= 100 ? { p99: sorted[Math.ceil(values.length * .99) - 1] } : {}) };
};
async function load(file, baseline = false) {
  const entry = path.join(baseline ? path.join(baselineRoot, 'source') : root, file);
  const output = await build({ entryPoints: [entry], bundle: true, write: false, platform: 'node', format: 'esm' });
  return import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
}
const original = await load('src/features/grid/layout/spatialIndex.ts', true);
const current = await load('src/features/grid/layout/spatialIndex.ts');
const layout = await load('src/features/grid/canvas/gridLayoutModel.ts');
const originalLayout = await load('src/features/grid/canvas/gridLayoutModel.ts', true);
const results = { baseline_source_hash: baselineManifest.sourceHash,
  scope: 'CPU-only production layout/index code; not browser paint, IPC, or image decoding', spatial: [], layout: [] };
for (const offset of [0, 4_000_000, 7_900_000]) {
  const positions = Array.from({ length: 1500 }, (_, i) => ({ x: i % 5 * 200, y: offset + Math.floor(i / 5) * 200, w: 190, h: 190 }));
  const timings = { baseline: [], candidate: [] };
  for (let i = 0; i < 450; i++) {
    for (const label of i % 2 ? ['baseline', 'candidate'] : ['candidate', 'baseline']) {
      const implementation = label === 'baseline' ? original : current;
      const start = performance.now();
      const index = implementation.buildTileSpatialIndex(positions);
      const found = index.queryYRange(offset + 400, offset + 1000, []);
      const elapsed = performance.now() - start;
      if (!found.includes(10)) throw new Error('Spatial benchmark missed visible tile');
      timings[label].push(elapsed);
    }
  }
  results.spatial.push({ offset, baseline_ms: stats(timings.baseline), candidate_ms: stats(timings.candidate) });
}
const items = Array.from({ length: 1500 }, (_, i) => ({ root_id: i + 1, kind: 'media', lifecycle: 'active',
  name: `Image ${i}`, cover_media_id: i + 1, content_hash: `hash-${i}`, mime: 'image/jpeg',
  width: 400 + i % 600, height: 600, duration_ms: null, frame_count: null, palette: [{ l: 50, a: 10, b: 10, weight: 1 }],
  imported_at_ms: i, captured_at_ms: null, modified_at_ms: i, media_count: 1, total_size_bytes: 1024, rating: 'unrated' }));
for (const viewMode of ['grid', 'waterfall', 'justified']) {
  const config = { width: 1200, targetSize: 200, gap: 16, viewMode, textHeight: 20, scrollbarWidth: 8 };
  const timings = Object.fromEntries(['baseline', 'candidate'].map(label => [label, { cold: [], prepare: [], commit: [] }]));
  for (let i = 0; i < 150; i++) {
    for (const label of i % 2 ? ['baseline', 'candidate'] : ['candidate', 'baseline']) {
    const implementation = label === 'baseline' ? originalLayout : layout;
    let start = performance.now();
    new implementation.GridLayoutRuntime().update(items, config);
    const coldMs = performance.now() - start;
    start = performance.now();
    implementation.prepareGridLayout(items, config);
    const prepareMs = performance.now() - start;
    start = performance.now();
    new implementation.GridLayoutRuntime().update(items, config);
    const commitMs = performance.now() - start;
    timings[label].cold.push(coldMs); timings[label].prepare.push(prepareMs); timings[label].commit.push(commitMs);
    }
  }
  results.layout.push({ viewMode, items: items.length, ...Object.fromEntries(Object.entries(timings).map(([label, times]) =>
    [label, { cold_commit_ms: stats(times.cold), preparation_ms: stats(times.prepare), prepared_commit_ms: stats(times.commit) }])) });
}
await mkdir(path.dirname(outputPath), { recursive: true });
await writeFile(outputPath, JSON.stringify(results, null, 2), { flag: 'wx' });
console.log(JSON.stringify({ outputPath, baseline_source_hash: results.baseline_source_hash }));
