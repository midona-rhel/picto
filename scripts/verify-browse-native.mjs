// One bounded, compiler-monitored verification process against a synthetic copy.
import { spawn } from 'node:child_process';
import { createReadStream } from 'node:fs';
import { copyFile, mkdir, realpath, stat, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { withCompilerGuard } from './compiler-idle-guard.mjs';

const [binaryArg, fixtureArg, outputArg, mode, count = '30'] = process.argv.slice(2);
if (!outputArg || !['matrix','cancellation','memory','history','imports'].includes(mode)) throw new Error('Expected binary, prepared synthetic SQLite file, new directory, matrix|cancellation|memory|history|imports, optional sample count');
const binary = await realpath(binaryArg), fixture = await realpath(fixtureArg);
if (!fixture.startsWith(path.resolve('artifacts/benchmarks/browse') + path.sep)
    || path.basename(path.dirname(fixture)) !== 'synthetic.library' || path.basename(fixture) !== 'library.sqlite') {
  throw new Error('Only prepared benchmark synthetic.library copies are allowed');
}
if (await stat(`${fixture}-wal`).then(s => s.size > 0,e => {if(e.code === 'ENOENT') return false; throw e;})) throw new Error('Source fixture has a nonempty WAL');
const output = path.resolve(outputArg);
await mkdir(path.dirname(output),{recursive:true}); await mkdir(output);
const database = path.join(output,'library.sqlite'); await copyFile(fixture,database);
const hash = async file => {
  const digest = createHash('sha256');
  for await (const chunk of createReadStream(file)) digest.update(chunk);
  return digest.digest('hex');
};
const report = {mode,binary,binary_sha256:await hash(binary),fixture,fixture_sha256:await hash(fixture),count:Number(count),records:[],deadline_ms:1200000};
report.selected_cases = process.env.PICTO_BROWSE_CASES?.split(',') ?? null;
await writeFile(path.join(output,'manifest.json'),JSON.stringify(report,null,2),{flag:'wx'});
await withCompilerGuard(output,async assertClear => {
  assertClear();
  const child = spawn(binary,[database,mode,count],{windowsHide:true,stdio:['ignore','pipe','pipe']});
  report.pid = child.pid;
  await writeFile(path.join(output,'processes.json'),JSON.stringify({pid:child.pid,deadline_ms:report.deadline_ms}),{flag:'wx'});
  console.log(JSON.stringify({mode,pid:child.pid,output}));
  let stdout = '',stderr = '',buffer = '',timedOut = false;
  child.stdout.on('data',data => {
    stdout += data; buffer += data;
    let end;
    while ((end=buffer.indexOf('\n'))>=0) {
      const line=buffer.slice(0,end).trim(); buffer=buffer.slice(end+1);
      const match=line.match(/^(CASE|CANCEL|SUMMARY|MEMORY|HISTORY) (.+)$/);
      if (match) report.records.push({kind:match[1],...JSON.parse(match[2])});
    }
  });
  child.stderr.on('data',data => {stderr += data;});
  const stop=()=>child.kill();
  const deadline=setTimeout(()=>{timedOut=true;stop();},report.deadline_ms);
  const heartbeat=setInterval(()=>console.log(JSON.stringify({pid:child.pid,records:report.records.length,status:'running'})),30000);
  process.once('SIGINT',stop);process.once('SIGTERM',stop);
  try { report.code=await new Promise((resolve,reject)=>{child.once('exit',resolve);child.once('error',reject);}); }
  finally {
    clearTimeout(deadline);clearInterval(heartbeat);process.off('SIGINT',stop);process.off('SIGTERM',stop);
    report.timed_out=timedOut;
    report.slower_than_50ms=report.records.filter(r=>r.kind==='CASE' && r.timing.p95_ms>50)
      .map(({sort,direction,filter,position,timing})=>({sort,direction,filter,position,p95_ms:timing.p95_ms,max_ms:timing.max_ms}));
    await writeFile(path.join(output,'probe.log'),stdout+stderr,{flag:'wx'});
    await writeFile(path.join(output,'report.json'),JSON.stringify(report,null,2),{flag:'wx'});
  }
  assertClear();
  if (report.code!==0 || timedOut) throw new Error('Verification failed; logs and all completed samples retained');
  console.log(JSON.stringify({records:report.records.length,over_50ms:report.slower_than_50ms}));
});
