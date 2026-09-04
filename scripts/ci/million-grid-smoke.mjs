// Read-only IPC/DOM audit of the visible million-item debug app. No simulated input.
import { spawnSync } from 'node:child_process';
const expression = `(${async function () {
  const invoke = async (command, args = {}) => {
    const started = performance.now();
    const raw = await window.picto.api.invoke(command, args);
    return { value: raw?.__pictoCoreJson ? JSON.parse(raw.__pictoCoreJson) : raw,
      roundTripMs: performance.now() - started, nativeMs: raw?.__pictoNativeMs };
  };
  const all = { scope: { kind: 'all' }, view: {
    filter: { kind: 'all', value: [] },
    sort: { field: 'imported_at', direction: 'descending', random_seed: null },
  }};
  const head = (await invoke('items.window', { query: all, window: { start: 0, limit: 4 } })).value;
  if (head.total !== 1000000) throw new Error('Expected the isolated million-item library');
  for (let i = 0; i < 2; i++) {
    const page = (await invoke('items.query', { query: all, page: { limit: 4, cursor: null } })).value;
    if (JSON.stringify(page.items) !== JSON.stringify(head.items)) throw new Error('Cached page and direct window disagree');
  }
  const samples = [];
  for (const start of [999999, 250000, 0]) {
    const result = await invoke('items.window', { query: all, window: { start, limit: 1500 } });
    if (result.value.items.length !== 1500 || result.value.total !== head.total) throw new Error('Incomplete grid window');
    samples.push({ start: result.value.start, roundTripMs: result.roundTripMs, nativeMs: result.nativeMs });
  }
  const navigation = (await invoke('navigation.get')).value;
  for (const folder of navigation.smart_folders) {
    const result = await invoke('items.window', { query: { ...all, scope: { kind: 'smart_folder', smart_folder_id: folder.smart_folder_id } }, window: { start: 999999, limit: 50 } });
    if (result.value.total > 0 && result.value.items.length === 0) throw new Error('Empty smart-folder window');
    samples.push({ smartFolder: folder.name, total: result.value.total, returned: result.value.items.length, roundTripMs: result.roundTripMs });
  }
  const verified = (await invoke('items.window', { query: all, window: { start: 0, limit: 4 } })).value;
  if (JSON.stringify(head.items) !== JSON.stringify(verified.items)) throw new Error('Query head changed after distant reads');
  const channel = crypto.randomUUID();
  const query = { ...all, view: { ...all.view,
    filter: { kind: 'clause', value: { clause: 'imported_at', minimum_ms: 0, maximum_ms: Date.now() + 1000000000000 } },
    sort: { field: 'name', direction: 'ascending', random_seed: null },
  }};
  const pending = invoke('items.window', { query, window: { start: 999999, limit: 1500 }, request: { channel, generation: 1 } })
    .then(() => 'completed', error => String(error));
  await new Promise(resolve => setTimeout(resolve, 5));
  const cancelledAt = performance.now();
  await invoke('items.supersede_window', { channel, generation: 2 });
  const outcome = await pending;
  const cancellationMs = performance.now() - cancelledAt;
  if (!outcome.includes('query superseded')) throw new Error('The obsolete query was not interrupted: ' + outcome);
  const next = await invoke('items.window', { query: all, window: { start: 500000, limit: 500 }, request: { channel, generation: 2 } });
  if (next.value.items.length !== 500) throw new Error('Cancellation affected the next query');
  let decodedThumbnails = 0;
  for (const item of head.items) {
    const response = await fetch('media://localhost/thumb/' + item.content_hash + '.jpg');
    if (!response.ok) throw new Error('Thumbnail unavailable');
    const bitmap = await createImageBitmap(await response.blob());
    if (bitmap.width === 0 || bitmap.height === 0) throw new Error('Empty decoded thumbnail');
    bitmap.close();
    decodedThumbnails++;
  }
  const canvasCount = document.querySelectorAll('[data-grid-scroll-container] canvas').length;
  if (!canvasCount) throw new Error('Grid is not mounted');
  return { total: head.total, samples, cancellationMs, decodedThumbnails, canvasCount, queryHeadStable: true };
}})()`;
console.log(`Read-only grid smoke PID ${process.pid}`);
const result = spawnSync(process.execPath, ['scripts/dev-cdp.mjs', 'eval', expression], { encoding: 'utf8', timeout: 60000 });
process.stdout.write(result.stdout || '');
process.stderr.write(result.stderr || '');
if (result.error) console.error(result.error);
process.exitCode = result.status ?? 1;
