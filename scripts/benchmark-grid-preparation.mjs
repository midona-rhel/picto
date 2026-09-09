import { spawn, spawnSync } from 'node:child_process';
import { mkdir, writeFile, readdir } from 'node:fs/promises';
import path from 'node:path';

const phase = process.argv[2];
if (!['baseline', 'candidate', 'cold-search', 'alphabetical'].includes(phase)) throw new Error('Expected baseline, candidate, cold-search, or alphabetical');
const directory = process.argv[3] ? path.resolve(process.argv[3]) : path.resolve('artifacts/benchmarks/grid-preparation', phase);
await mkdir(path.dirname(directory), { recursive: true });
await mkdir(directory); // Every run gets a new directory; previous samples are immutable.
const suffix = process.platform === 'win32' ? '.exe' : '';
let binaryDirectory = process.argv[4] ? path.resolve(process.argv[4]) : null;
let ftsExecutable;
if (!binaryDirectory) {
  // Finish every build before the first measurement. Compilation must not
  // compete with another benchmark or count towards a runtime comparison.
  const build = spawnSync('cargo', ['build', '--release', '--offline', '-p', 'picto_library', '--examples'], { stdio: 'inherit' });
  if (build.status !== 0) throw new Error('Benchmark build failed');
  const tests = spawnSync('cargo', ['test', '--release', '--offline', '-p', 'picto_library', '--test', 'fts_scale', '--no-run', '--message-format=json'], { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
  if (tests.status !== 0) throw new Error(tests.stderr || 'FTS benchmark build failed');
  const artifacts = tests.stdout.split('\n').filter(Boolean).map(line => { try { return JSON.parse(line); } catch { return null; } });
  ftsExecutable = artifacts.find(item => item?.target?.name === 'fts_scale' && item.executable)?.executable;
  binaryDirectory = path.resolve('target/release/examples');
} else {
  const files = await readdir(binaryDirectory);
  const tests = files.filter(file => /^fts_scale-[0-9a-f]+(?:\.exe)?$/.test(file));
  if (tests.length !== 1) throw new Error('Expected exactly one frozen FTS benchmark executable');
  ftsExecutable = path.join(binaryDirectory, tests[0]);
}
if (!ftsExecutable) throw new Error('FTS benchmark executable not found');
const executable = name => path.join(binaryDirectory, name + suffix);
const jobs = [
  ['grid-million', executable('grid_query_scale_probe'), ['10000', '100000', '1000000']],
  ['windows-million', executable('grid_window_scale_probe'), []],
  ['general-10000', executable('scale_probe'), ['10000']],
  ['smart-million', executable('smart_folder_scale_probe'), ['1000000', '1000']],
  ['smart-unique', executable('smart_folder_scale_probe'), ['100000', '1000', 'unique']],
  ['fts-200000', ftsExecutable, ['--ignored', '--nocapture']],
];
const results = [];
for (const [name, program, args] of jobs) {
  const started = Date.now();
  console.log(`START ${phase} ${name}`);
  const child = spawn(program, args, { env: { ...process.env, PICTO_FTS_SCALE_ITEMS: '200000' }, stdio: ['ignore', 'pipe', 'pipe'] });
  let timedOut = false;
  const deadline = setTimeout(() => { timedOut = true; child.kill(); }, 30 * 60_000);
  const stop = () => child.kill();
  process.once('SIGINT', stop); process.once('SIGTERM', stop);
  console.log(`PID ${name} ${child.pid}`);
  let stdout = '';
  let stderr = '';
  child.stdout.on('data', value => { stdout += value; });
  child.stderr.on('data', value => { stderr += value; });
  const heartbeat = setInterval(() => console.log(`RUNNING ${name} ${Math.round((Date.now() - started) / 1000)}s`), 30000);
  let code;
  try {
    code = await new Promise((resolve, reject) => { child.once('exit', resolve); child.once('error', reject); });
  } finally {
    clearInterval(heartbeat);
    clearTimeout(deadline);
    process.off('SIGINT', stop); process.off('SIGTERM', stop);
  }
  await writeFile(path.join(directory, `${name}.log`), stdout + stderr, { flag: 'wx' });
  results.push({ name, code, timedOut, pid: child.pid, elapsed_ms: Date.now() - started, program, args });
  await writeFile(path.join(directory, 'summary.json'), JSON.stringify(results, null, 2));
  console.log(`DONE ${name} code=${code} elapsed_ms=${Date.now() - started}`);
  if (code !== 0) { console.error(stderr); process.exitCode = 1; break; }
}
