// Bounded packaged-app test. Input stays inside the known synthetic library.
// Run only with explicit UI-test approval; no host-wide keyboard/mouse input.
import { spawn, spawnSync } from 'node:child_process';
import { mkdir, writeFile, realpath, cp, stat } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import { withCompilerGuard } from '../compiler-idle-guard.mjs';

if (!process.argv[2] || !process.argv[3]) throw new Error('Expected packaged executable and new run directory');
const root = process.cwd();
const executable = await realpath(process.argv[2]);
const output = path.resolve(process.argv[3]);
const sourceLibrary = await realpath(process.argv[4] ?? path.join(root, '.tmp/Million Grid Test.library'));
if (process.argv[4] && (!sourceLibrary.startsWith(path.join(root,'artifacts','benchmarks','browse') + path.sep)
    || path.basename(sourceLibrary) !== 'synthetic.library')) {
  throw new Error('Reopen tests may only copy a prior benchmark synthetic.library');
}
if (!executable.startsWith(path.join(root, 'artifacts') + path.sep)) throw new Error('Use an isolated local test package, not the installed application');
await mkdir(path.dirname(output), { recursive: true });
await mkdir(output);
if (await stat(path.join(sourceLibrary,'library.sqlite-wal')).then(s => s.size > 0,e => {if(e.code === 'ENOENT') return false; throw e;})) {
  throw new Error('Close and checkpoint the synthetic source library before copying it');
}
const library = path.join(output,'synthetic.library');
await cp(sourceLibrary,library,{recursive:true,force:false,errorOnExist:true});
const port = 9237;
try { await fetch(`http://127.0.0.1:${port}/json`); throw new Error('Test debugging port already occupied'); }
catch (error) { if (error.message === 'Test debugging port already occupied') throw error; }
await withCompilerGuard(output, async assertCompilerClear => {
assertCompilerClear();
const env = { ...process.env, PICTO_E2E: '1', PICTO_SMOKE_APP_DATA: path.join(output, 'profile'), PICTO_LIBRARY_ROOT: library, PICTO_OPEN_DEVTOOLS: '0' };
delete env.ELECTRON_RUN_AS_NODE;
const started = performance.now();
const child = spawn(executable, [`--remote-debugging-port=${port}`, '--remote-debugging-address=127.0.0.1'], { env, stdio: ['ignore', 'pipe', 'pipe'] });
let log = '', socket, send, closed = false, expired = false;
const traceEvents = [];
let traceComplete;
const exited = new Promise(resolve => {
  child.once('exit', (code, signal) => { closed = true; resolve({ code, signal }); });
  child.once('error', error => { closed = true; log += String(error); resolve({ error: String(error) }); });
});
child.stdout.on('data', data => { log += data; });
child.stderr.on('data', data => { log += data; });
const stop = () => {
  socket?.close();
  if (!closed && child.pid) {
    if (process.platform === 'win32') spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore', timeout: 10_000 });
    else child.kill();
  }
};
const deadline = setTimeout(() => { expired = true; stop(); }, 180_000);
process.once('SIGINT', stop); process.once('SIGTERM', stop);
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const report = { executable, library, sourceLibrary, pid: child.pid, launcher_pid: process.pid, deadline_ms: 180_000,
  startup_case: process.argv[4] ? 'reopen of a prepared synthetic copy; OS cache uncontrolled' : 'migration/recovery of the original synthetic fixture',
  kind: 'packaged synthetic-library smoke, not a statistical performance gate', checks: [] };
await writeFile(path.join(output, 'processes.json'), JSON.stringify(report, null, 2), { flag: 'wx' });
console.log(JSON.stringify({ pid: child.pid, output }));
try {
  let target;
  for (let i = 0; i < 600 && !expired && !closed; i++) {
    try { target = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()).find(page => page.type === 'page' && page.url.includes('/index.html')); } catch {}
    if (target) break;
    await delay(100);
  }
  if (!target) throw new Error('The owned test application did not expose its main page');
  const WebSocket = createRequire(import.meta.url)('ws');
  socket = new WebSocket(target.webSocketDebuggerUrl);
  socket.on('error', error => { log += '\nCDP: ' + String(error); });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', reject); });
  let sequence = 0;
  const pending = new Map();
  socket.on('message', data => {
    const value = JSON.parse(data);
    if (value.method === 'Tracing.dataCollected') traceEvents.push(...value.params.value);
    if (value.method === 'Tracing.tracingComplete') traceComplete?.();
    if (['Runtime.exceptionThrown','Runtime.consoleAPICalled','Log.entryAdded','Inspector.targetCrashed'].includes(value.method)) {
      (report.renderer_events ??= []).push({ms:performance.now()-started,...value});
    }
    const call = pending.get(value.id);
    if (call) { clearTimeout(call.timer); pending.delete(value.id); value.error ? call.reject(new Error(value.error.message)) : call.resolve(value.result); }
  });
  socket.on('close', () => { for (const call of pending.values()) {clearTimeout(call.timer); call.reject(new Error('Test connection closed'));} pending.clear(); });
  send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => {pending.delete(id); reject(new Error(`Timed out waiting for CDP ${method}`));},15000);
    pending.set(id, { resolve, reject, timer });
    socket.send(JSON.stringify({ id, method, params }));
  });
  await send('Runtime.enable');
  await send('Log.enable');
  const evaluate = async expression => {
    const value = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (value.exceptionDetails) throw new Error(JSON.stringify(value.exceptionDetails));
    return value.result.value;
  };
  const ready = async node => {
    let lastState;
    for (let i = 0; i < 240 && !expired; i++) {
      const state = await evaluate(`(() => {
        const view = document.querySelector('[data-view-phase]');
        const grid = document.querySelector('[data-grid-revision]');
        const canvas = document.querySelector('[data-grid-scroll-container] canvas');
        if (!view || !grid) return null;
        return { phase:view.dataset.viewPhase, node:view.dataset.viewNode, ...grid.dataset,
          has_canvas:Boolean(canvas?.width && canvas?.height), error:document.body.textContent.includes('Please retry') };
      })()`);
      lastState = state;
      const trace = report.transitions ??= [];
      if (JSON.stringify(trace.at(-1)?.state) !== JSON.stringify(state)) trace.push({ ms:performance.now() - started, state });
      if (state?.error) throw new Error('Retryable grid error: ' + JSON.stringify(state));
      if (state?.phase === 'idle' && state.has_canvas && Number(state.gridLoaded) > 0 && (!node || state.node === node)) return state;
      await delay(50);
    }
    throw new Error('Grid did not settle at ' + node + ': ' + JSON.stringify(lastState));
  };
  const initial = await ready('system:active');
  if (Number(initial.gridCount) !== 1_000_000) throw new Error('Not the expected synthetic million-item library');
  report.observed_grid_ready_upper_bound_ms = performance.now() - started;
  report.checks.push({ name: 'initial-grid', state: initial });
  // A fresh isolated profile can show the installed release notes. Dismiss
  // only that known dialog before driving navigation or judging screenshots.
  const dismissedNotes = await evaluate(`(() => {
    const done = document.querySelector('[data-modal-primary="true"]');
    if (done?.textContent.trim() !== 'Done') return false;
    done.click(); return true;
  })()`);
  report.dismissed_release_notes = dismissedNotes;
  await delay(250);
  await evaluate(`document.querySelector('[data-grid-scroll-container]').scrollTop = 1e10`);
  let tail;
  for (let i = 0; i < 200; i++) {
    tail = await ready('system:active');
    if (Number(tail.gridStart) > 990_000) break;
    await delay(50);
  }
  if (Number(tail.gridStart) <= 990_000) throw new Error('Tail scroll did not load a tail window');
  if (tail.gridPreview !== initial.gridPreview) throw new Error('Folder preview followed the resident tail instead of the query head');
  report.checks.push({ name: 'tail-jump-stable-preview', state: tail });
  for (const [cycle, pause] of [15, 0, 1, 40, 180, 15].entries()) {
    if (cycle > 0) {
      await evaluate(`document.querySelector('[data-help-id="sidebar-all"]').click()`);
      await ready('system:active');
    }
    await evaluate(`(async () => {
    const oldGrid = document.querySelector('[data-grid-scroll-container]');
    for (const id of ['sidebar-inbox', 'sidebar-all', 'sidebar-untagged']) {
      const row = document.querySelector('[data-help-id="' + id + '"]');
      if (!row) throw new Error('Missing navigation row ' + id);
      row.click(); oldGrid.scrollTop = oldGrid.scrollTop > 0 ? 0 : 1e10;
      await new Promise(resolve => setTimeout(resolve, ${pause}));
    }
  })()`);
    const destination = await ready('system:untagged');
    report.checks.push({ name: 'rapid-navigation-with-outgoing-scroll', cycle, pause_ms:pause, state: destination });
  }
  await evaluate(`document.querySelector('[data-help-id="sidebar-all"]').click()`);
  await ready('system:active');
  await send('Page.reload');
  await delay(500);
  const reloaded = await ready('system:active');
  report.checks.push({ name: 'renderer-reload', state: reloaded });
  if (process.argv.includes('--history')) {
    // Exercise the shipped preload/IPC/mutation/invalidation path, then wait
    // for the displayed grid to reflect each committed history operation.
    const invoke = (command, args = {}) => evaluate(`(async () => {
      const result = await window.picto.api.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)});
      return result && typeof result === 'object' && '__pictoCoreJson' in result
        ? JSON.parse(result.__pictoCoreJson) : result;
    })()`);
    const original = await invoke('items.details', {root_id: 1});
    const groupStarted = performance.now();
    const grouped = await invoke('items.organize_into_collection', {
      target: {kind: 'explicit', root_ids: [1, 2]}, cover_root_id: 1,
      winning_collection_id: null, name: 'Synthetic history smoke', notes: null,
    });
    const cycles = process.argv.includes('--history-repeat') ? 30 : 1;
    const operations = [[null, 999999], ['history.undo', 1000000],
      ...Array.from({length: cycles}, () => [['history.redo', 999999], ['history.undo', 1000000]]).flat()];
    for (const [operation, count] of operations) {
      const operationStarted = operation ? performance.now() : groupStarted;
      if (operation) await invoke(operation);
      let state;
      for (let attempt = 0; attempt < 200; attempt++) {
        state = await ready('system:active');
        if (Number(state.gridCount) === count) break;
        await delay(50);
      }
      if (Number(state.gridCount) !== count) throw new Error('History did not refresh the visible grid');
      report.checks.push({name: operation ?? 'items.organize_into_collection', state,
        operation_to_displayed_count_upper_bound_ms: performance.now() - operationStarted});
    }
    const restored = await invoke('items.details', {root_id: 1});
    if (JSON.stringify(restored.root) !== JSON.stringify(original.root)) {
      throw new Error('Packaged history replay changed the canonical original root');
    }
    report.history = {collection_id: grouped.collection_id, restored_root_id: restored.root.root_id,
      state: await invoke('history.state')};
    const measured = report.checks.filter(check => check.name === 'history.undo' || check.name === 'history.redo');
    // The initial undo establishes the starting state; retain it, but compare
    // the same 30 alternating undo/redo samples in both packaged builds.
    for (const operation of ['history.undo', 'history.redo']) {
      const samples = measured.filter(check => check.name === operation)
        .slice(operation === 'history.undo' ? 1 : 0)
        .map(check => check.operation_to_displayed_count_upper_bound_ms);
      const sorted = [...samples].sort((a,b) => a-b);
      report.history[operation] = {samples_ms: samples, n: samples.length,
        p50_ms: sorted[Math.ceil(sorted.length * 0.5)-1],
        ...(sorted.length >= 30 ? {p95_ms: sorted[Math.ceil(sorted.length * 0.95)-1]} : {}),
        max_ms: sorted.at(-1), scope: 'IPC operation start to observed idle grid with correct count; 50 ms polling upper bound, not native query time'};
    }
  }
  if (process.argv[5] === '--measure') {
    // Fresh sidebar navigation intentionally prepares 500 items. Scrolled
    // history restoration uses the actual production 1500-item window path.
    const scrollTail = async node => {
      await evaluate(`document.querySelector('[data-grid-scroll-container]').scrollTop = 1e10`);
      for (let attempt = 0; attempt < 200; attempt++) {
        const state = await ready(node);
        if (Number(state.gridStart)>0 && Number(state.gridLoaded)===1500) {await delay(100); return;}
        await delay(50);
      }
      throw new Error('History fixture did not reach a 1500-item tail window');
    };
    await scrollTail('system:active');
    await evaluate(`document.querySelector('[data-help-id="sidebar-untagged"]').click()`);
    await ready('system:untagged');
    await scrollTail('system:untagged');
    await send('Performance.enable');
    await send('Tracing.start', {categories:'toplevel,blink.user_timing,devtools.timeline',transferMode:'ReportEvents'});
    report.renderer_samples = [];
    for (let index = 0; index < 30; index++) {
      const target = index % 2 ? 'system:untagged' : 'system:active';
      const iconPath = index % 2 ? 'm9.5 3.5 4.5 4.5-4.5 4.5M14 8H2' : 'M6.5 3.5 2 8l4.5 4.5M2 8h12';
      const before = await send('Performance.getMetrics');
      await evaluate(`(() => { performance.clearMeasures('picto:grid-preparation-commit');
        const button=document.querySelector('path[d="${iconPath}"]')?.closest('button');
        if (!button || button.disabled) throw new Error('History navigation unavailable'); button.click(); })()`);
      const state = await ready(target);
      const measurements = await evaluate(`performance.getEntriesByName('picto:grid-preparation-commit').map(entry => ({duration_ms:entry.duration,...entry.detail}))`);
      const after = await send('Performance.getMetrics');
      if (measurements.length !== 1 || measurements[0].items !== 1500 || Number(state.gridStart)<=0) {
        throw new Error('Expected exactly one measured 1500-item commit: ' + JSON.stringify(measurements));
      }
      const metric = (result,name) => result.metrics.find(metric => metric.name === name)?.value;
      report.renderer_samples.push({index,target,state,...measurements[0],
        whole_transition_task_ms:1000*(metric(after,'TaskDuration')-metric(before,'TaskDuration')),
        renderer_js_heap_bytes:metric(after,'JSHeapUsedSize')});
    }
    const ordered = report.renderer_samples.map(sample => sample.duration_ms).sort((a,b)=>a-b);
    report.renderer_timing = {n:ordered.length,p50_ms:ordered[14],p95_ms:ordered[28],max_ms:ordered[29],
      scope:'Optimized opt-in instrumentation: layout preparation plus commit-to-first-valid-base-draw wall interval; upper bound on CPU, not compositor presentation',
      within_16ms_upper_bound:ordered[28]<=16};
    const completed = new Promise(resolve => { traceComplete = resolve; });
    await send('Tracing.end');
    await Promise.race([completed,delay(10000).then(() => {throw new Error('Renderer trace did not finish');})]);
    await writeFile(path.join(output,'renderer-trace.json'),JSON.stringify({traceEvents}),{flag:'wx'});
    console.log(JSON.stringify({renderer_timing:report.renderer_timing}));
  }
  const screenshot = await send('Page.captureScreenshot', { format: 'png' });
  await writeFile(path.join(output, 'grid.png'), Buffer.from(screenshot.data, 'base64'), { flag: 'wx' });
  const initializations = log.split('[main] library initialized in native core').length - 1;
  if (initializations !== 1) throw new Error(`Renderer reload initialized the backend ${initializations} times`);
  report.backend_initializations = initializations;
  report.pre_shutdown_no_library_errors = (log.match(/No library is open/g) ?? []).length;
  if (report.pre_shutdown_no_library_errors > 0) throw new Error('A live test view made a request without an open library');
  report.passed = true;
  try { await send('Browser.close'); } catch {}
  await Promise.race([exited, delay(3000)]);
} catch (error) {
  report.passed = false; report.error = String(error); process.exitCode = 1;
  if (send && socket?.readyState === 1) {
    try {
      const screenshot = await send('Page.captureScreenshot', { format:'png' });
      await writeFile(path.join(output, 'failure.png'), Buffer.from(screenshot.data, 'base64'), { flag:'wx' });
    } catch (captureError) { report.capture_error = String(captureError); }
    try { await send('Browser.close'); } catch {}
    await Promise.race([exited, delay(3000)]);
  }
} finally {
  clearTimeout(deadline); stop();
  process.off('SIGINT', stop); process.off('SIGTERM', stop);
  report.process_exit = await exited;
  report.timed_out = expired;
  if (expired || report.process_exit.code !== 0) {
    report.passed = false;
    report.error ??= 'Test application did not exit cleanly';
    process.exitCode = 1;
  }
  await writeFile(path.join(output, 'electron.log'), log, { flag: 'wx' });
  await writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2), { flag: 'wx' });
  console.log(JSON.stringify(report));
}
assertCompilerClear();
});
