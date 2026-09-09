// Isolated production comparison. Both existing probes create deterministic,
// disposable databases; no user library or concurrent compilation is involved.
import { spawn } from 'node:child_process';
import { mkdir, readFile, writeFile, realpath } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { withCompilerGuard } from './compiler-idle-guard.mjs';

const [baselineArg, candidateArg, outputArg, pairsArg = '3', countArg = '10000', mode = 'import'] = process.argv.slice(2);
if (!outputArg) throw new Error('Expected baseline scale_probe, candidate scale_probe and new output directory');
if (!['import', 'fts'].includes(mode)) throw new Error('Expected import or fts mode');
const pairs = Number(pairsArg);
if (!Number.isInteger(pairs) || pairs < 1 || pairs > 30) throw new Error('Use 1–30 alternating pairs');
const count = Number(countArg);
if (![10000,100000,1000000].includes(count)) throw new Error('Use a 10k, 100k or 1m synthetic import fixture');
const output = path.resolve(outputArg);
await mkdir(path.dirname(output), { recursive: true });
await mkdir(output);
const binaries = { baseline: await realpath(baselineArg), candidate: await realpath(candidateArg) };
const report = { fixture: mode === 'fts'
  ? `${count} deterministic synthetic imports in batches of 48, then FTS settlement in batches of 2000; independently created databases`
  : `${count} synthetic imports, maximum-sized batches, concurrent visible reader; independently created databases`, mode, count, runs: [], binaries: {} };
const durationMs = text => {
  const match = text?.match(/^([0-9.]+)(ns|µs|us|ms|s)$/);
  if (!match) throw new Error(`Unrecognized Rust duration: ${text}`);
  return Number(match[1]) * ({ ns: 0.000001, 'µs': 0.001, us: 0.001, ms: 1, s: 1000 }[match[2]]);
};
for (const [label, executable] of Object.entries(binaries)) {
  report.binaries[label] = { executable, sha256: createHash('sha256').update(await readFile(executable)).digest('hex') };
}
await writeFile(path.join(output,'manifest.json'), JSON.stringify(report,null,2), {flag:'wx'});
if (report.binaries.baseline.sha256 === report.binaries.candidate.sha256) {
  throw new Error('Baseline and candidate executables are identical; rebuild from the intended sources');
}
await withCompilerGuard(output, async assertCompilerClear => {
for (let pair = 0; pair < pairs; pair++) {
  for (const label of pair % 2 ? ['candidate','baseline'] : ['baseline','candidate']) {
    assertCompilerClear();
    const child = spawn(binaries[label], mode === 'fts'
      ? ['--ignored','--nocapture','--test-threads=1'] : [String(count)], {
      stdio:['ignore','pipe','pipe'], windowsHide:true,
      env: {...process.env, PICTO_FTS_SCALE_ITEMS: String(count)},
    });
    console.log(JSON.stringify({ pair, label, pid:child.pid }));
    let stdout = '', stderr = '', timedOut = false;
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    const stop = () => child.kill();
    const timeout = setTimeout(() => { timedOut = true; stop(); }, count === 1000000 ? 1800000 : 180000);
    const heartbeat = setInterval(() => console.log(JSON.stringify({pair,label,pid:child.pid,status:'running'})),30000);
    process.once('SIGINT',stop); process.once('SIGTERM',stop);
    let code;
    try { code = await new Promise((resolve,reject) => { child.once('error',reject); child.once('exit',resolve); }); }
    finally { clearTimeout(timeout); clearInterval(heartbeat); process.off('SIGINT',stop); process.off('SIGTERM',stop); }
    const logFile = `${pair}-${label}.log`;
    await writeFile(path.join(output,logFile),stdout+stderr,{flag:'wx'});
    const metrics = Object.fromEntries([...stdout.matchAll(/^([a-z_][a-z0-9_]*)=([0-9.]+)\r?$/gm)].map(([,key,value]) => [key,Number(value)]));
    if (mode === 'fts' && code === 0 && !timedOut) {
      const ingest = stdout.match(/ingest (\d+) items: (\S+)/);
      const settle = stdout.match(/settle fts \(~\d+ slots\): (\S+)/);
      metrics.roots = Number(ingest?.[1]);
      metrics.ingest_total_ms = durationMs(ingest?.[2]);
      metrics.fts_settle_ms = durationMs(settle?.[1]);
      metrics.import_and_settle_ms = metrics.ingest_total_ms + metrics.fts_settle_ms;
      const nameWrites = stdout.match(/^NAME_WRITES (.+)$/m);
      if (nameWrites) metrics.name_writes = JSON.parse(nameWrites[1]);
    }
    report.runs.push({pair,label,pid:child.pid,code,timedOut,logFile,metrics});
    await writeFile(path.join(output,'report.json'),JSON.stringify(report,null,2));
    assertCompilerClear();
    if (code !== 0 || timedOut || !metrics.ingest_total_ms || metrics.roots !== count) throw new Error(`${label} failed or used the wrong fixture size; all output retained`);
  }
}
report.comparisons = Array.from({length:pairs}, (_,pair) => {
  const value = label => report.runs.find(run => run.pair === pair && run.label === label).metrics.ingest_total_ms;
  const baseline = value('baseline'), candidate = value('candidate');
  return {pair,baseline_ms:baseline,candidate_ms:candidate,delta_ms:candidate-baseline,delta_percent:100*(candidate/baseline-1)};
});
if (mode === 'fts') {
  report.fts_comparisons = ['fts_settle_ms','import_and_settle_ms'].flatMap(metric =>
    Array.from({length:pairs}, (_,pair) => {
      const value = label => report.runs.find(run => run.pair === pair && run.label === label).metrics[metric];
      const baseline = value('baseline'), candidate = value('candidate');
      return {metric,pair,baseline_ms:baseline,candidate_ms:candidate,delta_ms:candidate-baseline,delta_percent:100*(candidate/baseline-1)};
    }));
}
const measured = mode === 'fts' ? ['ingest_total_ms','fts_settle_ms','import_and_settle_ms'] : ['ingest_total_ms'];
report.summary = Object.fromEntries(['baseline','candidate'].map(label => [label,
  Object.fromEntries(measured.map(metric => {
    const samples = report.runs.filter(run => run.label === label).map(run => run.metrics[metric]).toSorted((a,b) => a-b);
    return [metric,{n:samples.length,p50_ms:samples[Math.ceil(samples.length*.5)-1],
      ...(samples.length >= 30 ? {p95_ms:samples[Math.ceil(samples.length*.95)-1]} : {}),max_ms:samples.at(-1)}];
  })),
]));
await writeFile(path.join(output,'report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify(report.comparisons));
if (report.fts_comparisons) console.log(JSON.stringify(report.fts_comparisons));
});
