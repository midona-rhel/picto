// Repeatability investigation using already-built binaries, never compiling
// during measurements. Each invocation owns one bounded native child at a time.
import { spawn } from 'node:child_process';
import { mkdir, readFile, writeFile, stat, realpath, copyFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { withCompilerGuard } from './compiler-idle-guard.mjs';

const [baselineArg, candidateArg, fixtureArg, outputArg, pairsArg = '3', mode = 'windows'] = process.argv.slice(2);
if (!['windows','cold-search'].includes(mode)) throw new Error('Expected windows or cold-search mode');
if (!outputArg) throw new Error('Expected baseline binary, candidate binary, synthetic SQLite fixture, and new run directory');
const pairs = Number(pairsArg);
if (!Number.isInteger(pairs) || pairs < 1 || pairs > 10) throw new Error('Use one to ten alternating pairs');
const binaries = { baseline: await realpath(baselineArg), candidate: await realpath(candidateArg) };
const fixture = await realpath(fixtureArg);
if (!fixture.startsWith(path.resolve('.tmp') + path.sep)) throw new Error('Only repository synthetic .tmp fixtures are allowed');
if (await stat(`${fixture}-wal`).then(s => s.size > 0, e => { if (e.code === 'ENOENT') return false; throw e; })) {
  throw new Error('Fixture has a WAL; close and checkpoint the synthetic library before making independent copies');
}
const output = path.resolve(outputArg);
await mkdir(path.dirname(output), { recursive: true });
await mkdir(output);
const hash = async file => createHash('sha256').update(await readFile(file)).digest('hex');
const report = {
  mode,
  purpose: 'Alternating window-regression investigation; existing probes are not the complete acceptance matrix',
  fixture, fixture_sha256: await hash(fixture), fixture_bytes: (await stat(fixture)).size,
  binaries: await Promise.all(Object.entries(binaries).map(async ([label, executable]) => ({ label, executable, sha256: await hash(executable) }))),
  runs: [],
};
await writeFile(path.join(output, 'manifest.json'), JSON.stringify(report, null, 2), { flag: 'wx' });
await withCompilerGuard(output, async assertCompilerClear => {
for (let pair = 0; pair < pairs; pair++) {
  for (const label of pair % 2 ? ['candidate', 'baseline'] : ['baseline', 'candidate']) {
    const runFixture = path.join(output, `${pair}-${label}.sqlite`);
    // Each binary receives identical canonical starting data. A candidate schema
    // migration must never make a later baseline run open the candidate schema.
    await copyFile(fixture, runFixture);
    assertCompilerClear();
    const child = spawn(binaries[label], [runFixture], { stdio: ['ignore', 'pipe', 'pipe'] });
    console.log(JSON.stringify({ pair, label, pid: child.pid }));
    let stdout = '', stderr = '', timedOut = false;
    const heartbeat = setInterval(() => console.log(JSON.stringify({pair,label,pid:child.pid,status:'running'})),30000);
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    const stop = () => child.kill();
    const timeout = setTimeout(() => { timedOut = true; stop(); }, mode === 'cold-search' ? 600_000 : 120_000);
    process.once('SIGINT', stop); process.once('SIGTERM', stop);
    const began = performance.now();
    let code;
    try { code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', resolve); }); }
    finally { clearTimeout(timeout); clearInterval(heartbeat); process.off('SIGINT', stop); process.off('SIGTERM', stop); }
    const logFile = `${pair}-${label}.log`;
    await writeFile(path.join(output, logFile), stdout + stderr, { flag: 'wx' });
    const prefix = mode === 'cold-search' ? 'COLD_SEARCH ' : 'CASE ';
    const cases = stdout.split(/\r?\n/).filter(line => line.startsWith(prefix)).map(line => JSON.parse(line.slice(prefix.length)));
    const preparation = stdout.split(/\r?\n/).filter(line => line.startsWith('PREPARE ')).map(line => JSON.parse(line.slice(8)));
    report.runs.push({ pair, label, pid: child.pid, code, timedOut, elapsed_ms: performance.now() - began, logFile, runFixture, preparation, cases });
    await writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2));
    assertCompilerClear();
    if (code !== 0 || timedOut) throw new Error(`${label} failed; incomplete run and logs retained`);
  }
}
report.comparisons = mode === 'cold-search' ? report.runs[0].cases.map((sample,index) => ({
  sort:sample.sort,query:sample.query,requested_start:sample.requested_start,
  paired_first_requests:Array.from({length:pairs}, (_,pair) => {
    const value = label => report.runs.find(run => run.pair === pair && run.label === label).cases[index];
    const baseline=value('baseline'), candidate=value('candidate');
    if (baseline.sort!==candidate.sort || baseline.query!==candidate.query || baseline.requested_start!==candidate.requested_start) throw new Error('Cold workloads differ');
    return {pair,baseline_ms:baseline.ms,candidate_ms:candidate.ms,delta_ms:candidate.ms-baseline.ms,delta_percent:100*(candidate.ms/baseline.ms-1)};
  })
})) : report.runs[0].cases.map(({ case: name }) => ({
  case: name,
  paired_warm_p95: Array.from({ length: pairs }, (_, pair) => {
    const value = label => report.runs.find(run => run.pair === pair && run.label === label).cases.find(item => item.case === name).warm_jumps.p95_ms;
    const baseline = value('baseline'), candidate = value('candidate');
    return { pair, baseline_ms: baseline, candidate_ms: candidate, delta_ms: candidate - baseline, delta_percent: 100 * (candidate / baseline - 1) };
  }),
}));
await writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report.comparisons));
});
