// Observe compiler activity without starting a new PowerShell process for each
// sample. Never stop somebody else's compiler; invalidate the measurement.
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { cpus, freemem } from 'node:os';

const execute = promisify(execFile);
const compilerName = /^(rustc|cargo|cl|link|lld-link|clang(?:\+\+)?|gcc|g\+\+|cc1|msbuild|cmake|ninja)(?:\.exe)?$/i;

export async function withCompilerGuard(output, operation) {
  const report = { interval_ms: 1000, required_quiet_samples: 5,
    scope: 'Sampled compiler/build-process absence, not proof of an otherwise idle host or OS-cold caches',
    samples: [], passed: false };
  let monitor, buffer = '', stopped = false, failure, quiet = 0;
  const cpuTotals = () => cpus().reduce((sum,cpu) => ({
    idle:sum.idle+cpu.times.idle, total:sum.total+Object.values(cpu.times).reduce((a,b)=>a+b,0),
  }),{idle:0,total:0});
  let previousCpu = cpuTotals();
  let wake;
  const ready = new Promise(resolve => { wake = resolve; });
  const accept = processes => {
    const currentCpu = cpuTotals();
    const elapsed = currentCpu.total-previousCpu.total;
    const cpuBusyPercent = elapsed > 0 ? 100*(1-(currentCpu.idle-previousCpu.idle)/elapsed) : null;
    previousCpu = currentCpu;
    report.samples.push({ at: new Date().toISOString(), processes, cpu_busy_percent:cpuBusyPercent, free_memory_bytes:freemem() });
    if (processes.length) failure ??= `Compiler activity detected: ${JSON.stringify(processes)}`;
    quiet = processes.length ? 0 : quiet + 1;
    if (failure || quiet >= report.required_quiet_samples) wake();
  };
  const failed = error => { failure ??= String(error); wake(); };
  const interrupted = () => failed('Measurement interrupted');
  process.once('SIGINT',interrupted); process.once('SIGTERM',interrupted);
  const startupTimeout = setTimeout(() => failed('Compiler monitor did not become ready'),15000);
  let polling;
  if (process.platform === 'win32') {
    const command = `while ($true) {
      $found = @([System.Diagnostics.Process]::GetProcesses() | ForEach-Object {
        try { if ($_.ProcessName -match '^(rustc|cargo|cl|link|lld-link|clang|clang\\+\\+|gcc|g\\+\\+|cc1|msbuild|cmake|ninja)$') {
          @{ pid = $_.Id; name = $_.ProcessName }
        } } finally { $_.Dispose() }
      });
      [Console]::WriteLine((ConvertTo-Json -InputObject $found -Compress));
      Start-Sleep -Milliseconds 1000
    }`;
    monitor = spawn('powershell.exe',['-NoLogo','-NoProfile','-NonInteractive','-Command',command],
      { windowsHide:true, stdio:['ignore','pipe','pipe'] });
    report.monitor_pid = monitor.pid;
    monitor.on('error',failed);
    monitor.on('exit',code => { if (!stopped) failed(`Compiler monitor exited: ${code}`); });
    monitor.stderr.on('data',data => failed(data.toString()));
    monitor.stdout.on('data',data => {
      buffer += data;
      let end;
      while ((end = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0,end).trim(); buffer = buffer.slice(end+1);
        try { if (line) accept(JSON.parse(line)); } catch (error) { failed(error); }
      }
    });
  } else {
    polling = (async () => {
      while (!stopped) {
        try {
          const {stdout} = await execute('ps',['-axo','pid=,comm='],{timeout:5000});
          accept(stdout.trim().split('\n').flatMap(line => {
            const match = line.trim().match(/^(\d+)\s+(.+)$/);
            const name = match && path.basename(match[2]);
            return name && compilerName.test(name) ? [{pid:Number(match[1]),name}] : [];
          }));
        } catch (error) { failed(error); }
        await new Promise(resolve => setTimeout(resolve,1000));
      }
    })();
  }
  const assertClear = () => { if (failure) throw new Error(failure); };
  try {
    await ready;
    clearTimeout(startupTimeout);
    assertClear();
    await operation(assertClear);
    assertClear();
    report.passed = true;
  } finally {
    process.off('SIGINT',interrupted); process.off('SIGTERM',interrupted);
    clearTimeout(startupTimeout);
    stopped = true;
    if (monitor && monitor.exitCode === null) {
      const closed = new Promise(resolve => monitor.once('exit',resolve));
      monitor.kill();
      await closed;
    }
    await polling;
    report.failure = failure ?? null;
    await writeFile(path.join(output,'compiler-activity.json'),JSON.stringify(report,null,2),{flag:'wx'});
  }
}
