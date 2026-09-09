// Production native process-cold profiling, not packaged first paint or OS-cold.
// Supply a prepared synthetic copy with a valid checkpoint, never a real library.
import { spawn } from 'node:child_process';
import { createReadStream } from 'node:fs';
import { copyFile, mkdir, realpath, stat, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { withCompilerGuard } from './compiler-idle-guard.mjs';

const [binaryArg, fixtureArg, outputArg, countArg = '30'] = process.argv.slice(2);
const prepare = process.argv.includes('--prepare');
if (!outputArg) throw new Error('Expected native binary, prepared synthetic SQLite file, new output directory, optional sample count');
const count = Number(countArg);
if (!Number.isInteger(count) || count < 1 || count > 100) throw new Error('Use 1–100 samples');
if (prepare && count !== 1) throw new Error('Preparation runs exactly once and is not a normal-startup measurement');
const binary = await realpath(binaryArg), fixture = await realpath(fixtureArg);
if (!fixture.startsWith(path.resolve('artifacts/benchmarks/browse') + path.sep)
    || path.basename(path.dirname(fixture)) !== 'synthetic.library'
    || path.basename(fixture) !== 'library.sqlite') {
  throw new Error('Only prepared benchmark synthetic.library copies are allowed');
}
if (await stat(`${fixture}-wal`).then(s => s.size > 0, e => { if (e.code === 'ENOENT') return false; throw e; })) {
  throw new Error('Close and checkpoint the synthetic fixture before copying');
}
const output = path.resolve(outputArg);
await mkdir(path.dirname(output), { recursive: true });
await mkdir(output);
const databaseDirectory = prepare ? path.join(output, 'synthetic.library') : output;
if (prepare) await mkdir(databaseDirectory);
const database = path.join(databaseDirectory, 'library.sqlite');
await copyFile(fixture, database);
const hash = async file => {
  const digest = createHash('sha256');
  for await (const chunk of createReadStream(file)) digest.update(chunk);
  return digest.digest('hex');
};
const report = {
  kind: prepare ? 'Isolated synthetic projection migration/recovery preparation, not normal startup'
    : 'Native release process-cold startup; OS cache uncontrolled, not packaged first paint',
  binary, binary_sha256: await hash(binary), fixture, fixture_sha256: await hash(fixture),
  samples: [],
};
await writeFile(path.join(output, 'manifest.json'), JSON.stringify(report, null, 2), { flag: 'wx' });
await withCompilerGuard(output, async assertClear => {
  for (let index = 0; index < count; index++) {
    assertClear();
    const started = performance.now();
    const child = spawn(binary, [database, '--startup-only'], { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    console.log(JSON.stringify({ index, pid: child.pid }));
    let stdout = '', stderr = '', timedOut = false;
    child.stdout.on('data', data => { stdout += data; });
    child.stderr.on('data', data => { stderr += data; });
    const stop = () => child.kill();
    const deadline = setTimeout(() => { timedOut = true; stop(); }, prepare ? 120000 : 30000);
    process.once('SIGINT', stop); process.once('SIGTERM', stop);
    let code;
    try {
      code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', resolve); });
    } finally {
      clearTimeout(deadline); process.off('SIGINT', stop); process.off('SIGTERM', stop);
    }
    const elapsed_ms = performance.now() - started;
    await writeFile(path.join(output, `${index}.log`), stdout + stderr, { flag: 'wx' });
    const line = stdout.split(/\r?\n/).find(line => line.startsWith('STARTUP '));
    const result = line ? JSON.parse(line.slice(8)) : null;
    report.samples.push({ index, pid: child.pid, code, timedOut, elapsed_ms, result });
    await writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2));
    assertClear();
    if (code !== 0 || timedOut || !result) throw new Error('Startup probe failed; incomplete samples retained');
    if (!prepare && (!result.projection.checkpoint_reused)) throw new Error('This is recovery, not a normal reopen; sample retained separately');
    if (result.total !== 1000000 || result.items !== 1500) throw new Error('Expected renderable million-item fixture');
  }
});
const distribution = values => {
  const sorted = values.toSorted((a, b) => a - b);
  return { samples: sorted.length, p50: sorted[Math.ceil(sorted.length * 0.5) - 1],
    ...(sorted.length >= 30 ? {p95: sorted[Math.ceil(sorted.length * 0.95) - 1]} : {}), max: sorted.at(-1) };
};
report.summary = {
  open_ms: distribution(report.samples.map(s => s.result.open_ms)),
  first_window_ms: distribution(report.samples.map(s => s.result.first_window_ms)),
  projection: Object.fromEntries(Object.keys(report.samples[0].result.projection)
    .filter(key => key.endsWith('_us')).map(key => [key, distribution(report.samples.map(s => s.result.projection[key]))])),
};
await writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report.summary));
