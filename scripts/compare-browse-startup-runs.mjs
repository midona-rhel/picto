// Alternating process-cold opens of already prepared, canonical-equivalent
// synthetic fixtures. Migration/recovery is deliberately a separate run.
import { spawn } from 'node:child_process';
import { createReadStream } from 'node:fs';
import { copyFile, mkdir, realpath, stat, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { withCompilerGuard } from './compiler-idle-guard.mjs';

const [beforeExe, afterExe, beforeFixture, afterFixture, outputArg, countArg = '30'] = process.argv.slice(2);
const count = Number(countArg);
if (!outputArg || !Number.isInteger(count) || count < 30 || count > 100) {
  throw new Error('Expected baseline/candidate binaries, baseline/candidate prepared synthetic databases, new output, and 30–100 pairs');
}
const output = path.resolve(outputArg);
await mkdir(path.dirname(output), {recursive:true});
await mkdir(output);
const hash = async file => {
  const digest = createHash('sha256');
  for await (const bytes of createReadStream(file)) digest.update(bytes);
  return digest.digest('hex');
};
const runs = {};
for (const [label, executable, source] of [['baseline',beforeExe,beforeFixture],['candidate',afterExe,afterFixture]]) {
  const binary = await realpath(executable), fixture = await realpath(source);
  if (!fixture.startsWith(path.resolve('artifacts/benchmarks/browse') + path.sep)
      || path.basename(path.dirname(fixture)) !== 'synthetic.library' || path.basename(fixture) !== 'library.sqlite') {
    throw new Error('Only prepared benchmark synthetic.library copies are allowed');
  }
  if (await stat(`${fixture}-wal`).then(s => s.size > 0,e => {if(e.code === 'ENOENT') return false; throw e;})) {
    throw new Error('Close/checkpoint the synthetic source before copying');
  }
  const database = path.join(output,`${label}.sqlite`);
  await copyFile(fixture,database);
  runs[label] = {binary,fixture,database,binary_sha256:await hash(binary),fixture_sha256:await hash(fixture)};
}
const report = {kind:'Alternating native release process-cold opens; OS cache uncontrolled, not OS-cold or packaged first paint',
  count,deadline_per_child_ms:30000,runs,samples:[]};
await writeFile(path.join(output,'manifest.json'),JSON.stringify(report,null,2),{flag:'wx'});
await withCompilerGuard(output,async assertClear => {
  for (let pair = 0; pair < count; pair++) {
    for (const label of pair % 2 ? ['candidate','baseline'] : ['baseline','candidate']) {
      assertClear();
      const child = spawn(runs[label].binary,[runs[label].database,'--startup-only'],{windowsHide:true,stdio:['ignore','pipe','pipe']});
      console.log(JSON.stringify({pair,label,pid:child.pid}));
      let stdout='',stderr='',timedOut=false;
      const stop=()=>child.kill();
      const deadline=setTimeout(()=>{timedOut=true;stop();},report.deadline_per_child_ms);
      process.once('SIGINT',stop);process.once('SIGTERM',stop);
      child.stdout.on('data',data=>{stdout+=data;});child.stderr.on('data',data=>{stderr+=data;});
      let code,error;
      try { code=await new Promise((resolve,reject)=>{child.once('exit',resolve);child.once('error',reject);}); }
      catch (failure) {error=String(failure);}
      finally {clearTimeout(deadline);process.off('SIGINT',stop);process.off('SIGTERM',stop);}
      const logFile=`${pair}-${label}.log`;
      await writeFile(path.join(output,logFile),stdout+stderr,{flag:'wx'});
      const line=stdout.split(/\r?\n/).find(line=>line.startsWith('STARTUP '));
      let result;
      try {result=line?JSON.parse(line.slice(8)):null;} catch (failure) {error=String(failure);}
      report.samples.push({pair,label,pid:child.pid,code,error,timedOut,logFile,result});
      await writeFile(path.join(output,'report.json'),JSON.stringify(report,null,2));
      assertClear();
      if (code!==0 || timedOut || !result) throw new Error('Startup failed; all samples and logs retained');
      if (!result.projection.checkpoint_reused) throw new Error('Unexpected recovery; do not classify this as normal startup');
      if (result.total!==1000000 || result.items!==1500) throw new Error('Wrong synthetic fixture');
    }
  }
});
const distribution = values => {
  const sorted=values.toSorted((a,b)=>a-b);
  return {n:sorted.length,p50_ms:sorted[Math.ceil(sorted.length*.5)-1],p95_ms:sorted[Math.ceil(sorted.length*.95)-1],max_ms:sorted.at(-1)};
};
report.summary=Object.fromEntries(['baseline','candidate'].map(label=>{
  const values=report.samples.filter(sample=>sample.label===label).map(sample=>sample.result);
  return [label,{open:distribution(values.map(r=>r.open_ms)),first_window:distribution(values.map(r=>r.first_window_ms)),
    text_loading:distribution(values.map(r=>r.projection.text_load_us/1000)),
    name_loading:distribution(values.map(r=>r.projection.name_load_us/1000))}];
}));
report.deltas=Object.fromEntries(Object.keys(report.summary.baseline).map(metric=>{
  const before=report.summary.baseline[metric].p95_ms,after=report.summary.candidate[metric].p95_ms;
  return [metric,{p95_delta_ms:after-before,p95_delta_percent:100*(after/before-1)}];
}));
await writeFile(path.join(output,'report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({summary:report.summary,deltas:report.deltas}));
