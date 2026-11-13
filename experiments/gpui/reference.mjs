#!/usr/bin/env node
// Plays a script into the Electron app over the Chrome DevTools protocol and
// saves pictures, taking the same lines as the GPUI off-screen test
// (`src/offscreen.rs`), so one script photographs a view in both apps:
//
//   move X Y | down X Y [clicks] | up X Y [clicks] | drag X Y | rdown X Y | rup X Y
//   scroll X Y DISTANCE | key <keystroke> | type <text> | wait <ms>
//   shot <name> | size W H | js <expression> | reload | window <n> | pace <ms>
//   dragfiles X Y PATH... | drop X Y PATH... | dragexit
//
// `size` sets the viewport in points (pictures are twice that), `js` prints
// an expression's value, and `reload` loads the window again so that a
// script starts where a freshly opened app does (the GPUI test opens the
// app anew each run and ignores the line). `window <n>` sends what follows
// to the app's nth window: 0 is the main one, the others are numbered in the
// order they opened during the script. `dragfiles` brings files from the
// system over the window through the browser's own drag events, `drop` also
// drops them there (which imports them into the reference's copy), and
// `dragexit` cancels the drag. The app must be a development build started with
// its automation switches on a disposable library copy; see the README.
//
//   node reference.mjs steps.txt outdir            (PICTO_REFERENCE_PORT, default 9222)
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';

const [script, out] = process.argv.slice(2);
mkdirSync(out, { recursive: true });
const port = process.env.PICTO_REFERENCE_PORT || '9222';
// The app's windows are the pages served from the renderer's own address.
const pages = async () =>
  (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).filter(
    (target) => target.type === 'page' && /^http:\/\/(127\.0\.0\.1|localhost)/.test(target.url),
  );
// Windows in the order this script came to know them: the main one first
// (the page at the renderer's root), then each new one as it appears.
const known = [];
async function refresh() {
  const found = await pages();
  if (known.length === 0) {
    // Other windows load their own pages (`settings.html`, …).
    const main = found.find((target) => ['/', '/index.html'].includes(new URL(target.url).pathname)) ?? found[0];
    if (!main) throw new Error('reference app not found');
    known.push(main.id);
  }
  for (const target of found) if (!known.includes(target.id)) known.push(target.id);
  return found;
}
let socket;
let next = 0;
const pending = new Map();
async function attach(index) {
  const found = await refresh();
  const target = found.find((candidate) => candidate.id === known[index]);
  if (!target) throw new Error(`there are ${known.length} windows`);
  socket?.close();
  socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.onopen = resolve;
    socket.onerror = reject;
  });
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    if (message.id && pending.has(message.id)) {
      const { resolve, reject } = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) reject(new Error(JSON.stringify(message.error)));
      else resolve(message.result);
    }
  };
}
await attach(0);
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = ++next;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
// Makes the page draw a frame now, whether or not its window is showing.
const frame = () =>
  send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true }).catch(() => {});
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
// A window that is not in front drops presses and keys and throttles its
// timers: have the page behave as the focused, visible one while a script
// plays.
const wake = async () => {
  await send('Emulation.setFocusEmulationEnabled', { enabled: true }).catch(() => {});
  await send('Page.setWebLifecycleState', { state: 'active' }).catch(() => {});
};
await wake();
const mouse = (type, x, y, button = 'none', clickCount = 0, extra = {}) =>
  send('Input.dispatchMouseEvent', { type, x, y, button, clickCount, ...extra });

const KEYS = {
  space: [' ', 'Space', 32, ' '],
  escape: ['Escape', 'Escape', 27],
  enter: ['Enter', 'Enter', 13, '\r'],
  backspace: ['Backspace', 'Backspace', 8],
  delete: ['Delete', 'Delete', 46],
  tab: ['Tab', 'Tab', 9],
  left: ['ArrowLeft', 'ArrowLeft', 37],
  right: ['ArrowRight', 'ArrowRight', 39],
  up: ['ArrowUp', 'ArrowUp', 38],
  down: ['ArrowDown', 'ArrowDown', 40],
  '-': ['-', 'Minus', 189, '-'],
  '=': ['=', 'Equal', 187, '='],
};
async function key(stroke) {
  // The minus key is the one name that holds the separator (`cmd--`).
  const minus = stroke.endsWith('-');
  const parts = (minus ? stroke.slice(0, -1) : stroke).split('-');
  const name = minus ? (parts.pop(), '-') : parts.pop();
  let modifiers = 0;
  for (const part of parts) {
    modifiers |= { alt: 1, ctrl: 2, cmd: 4, shift: 8 }[part] ?? 0;
  }
  const known = KEYS[name];
  const [keyName, code, keyCode, text] = known ?? [
    name,
    /^[a-z]$/.test(name) ? `Key${name.toUpperCase()}` : /^[0-9]$/.test(name) ? `Digit${name}` : name,
    name.toUpperCase().charCodeAt(0),
    name.length === 1 ? name : undefined,
  ];
  // No native key code: a Windows one given as the Mac's is another key there
  // (65, the A key's, is one the app treats as Settings).
  const base = { key: keyName, code, windowsVirtualKeyCode: keyCode, modifiers };
  // With Command or Control held the key is a shortcut, not text.
  const typed = modifiers & 6 ? undefined : text;
  await send('Input.dispatchKeyEvent', { type: typed ? 'keyDown' : 'rawKeyDown', ...base, text: typed });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
}

// `pace <ms>`: how long to wait after each input line (200 ms unless set).
let pace = 200;
for (const raw of readFileSync(script, 'utf8').split('\n')) {
  const line = raw.trim();
  if (!line || line.startsWith('#')) continue;
  const [kind, ...rest] = line.split(' ');
  const numbers = rest.map(Number);
  if (kind === 'wait') {
    // A window behind others makes no frames on its own; ask for one every
    // 250 ms so that its observers, transitions and images keep running.
    for (let left = numbers[0]; left > 0; left -= 250) {
      await frame();
      await sleep(Math.min(250, left));
    }
  }
  else if (kind === 'pace') pace = numbers[0];
  else if (kind === 'shot') {
    const { data } = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true });
    writeFileSync(path.join(out, `${rest[0]}.png`), Buffer.from(data, 'base64'));
  } else if (kind === 'size') {
    await send('Emulation.setDeviceMetricsOverride', { width: numbers[0], height: numbers[1], deviceScaleFactor: 2, mobile: false });
    await sleep(500);
  } else if (kind === 'window') {
    await attach(numbers[0]);
    await wake();
  } else if (kind === 'reload') {
    await send('Page.reload');
    for (let step = 0; step < 16; step++) {
      await sleep(250);
      await frame();
    }
  } else if (kind === 'js') {
    const result = await send('Runtime.evaluate', { expression: rest.join(' '), returnByValue: true, awaitPromise: true });
    console.log(JSON.stringify(result.result?.value ?? result.exceptionDetails?.text ?? null));
  } else if (kind === 'move') await mouse('mouseMoved', numbers[0], numbers[1]);
  // A move with the left button held, as a drag delivers it.
  else if (kind === 'drag') await mouse('mouseMoved', numbers[0], numbers[1], 'left', 0, { buttons: 1 });
  else if (kind === 'down') await mouse('mousePressed', numbers[0], numbers[1], 'left', numbers[2] || 1);
  else if (kind === 'up') await mouse('mouseReleased', numbers[0], numbers[1], 'left', numbers[2] || 1);
  else if (kind === 'rdown') await mouse('mousePressed', numbers[0], numbers[1], 'right', 1);
  else if (kind === 'rup') await mouse('mouseReleased', numbers[0], numbers[1], 'right', 1);
  else if (kind === 'scroll')
    await mouse('mouseWheel', numbers[0], numbers[1], 'none', 0, { deltaX: 0, deltaY: -numbers[2] });
  // Files from the system dragged over the window and dropped on it, through
  // the browser's own drag events, so the page receives them as Finder's do.
  else if (kind === 'dragfiles' || kind === 'drop') {
    const data = { items: [], files: rest.slice(2), dragOperationsMask: 1 };
    const at = { x: numbers[0], y: numbers[1], data };
    await send('Input.dispatchDragEvent', { type: 'dragEnter', ...at });
    await send('Input.dispatchDragEvent', { type: 'dragOver', ...at });
    if (kind === 'drop') await send('Input.dispatchDragEvent', { type: 'drop', ...at });
  } else if (kind === 'dragexit') await send('Input.dispatchDragEvent', { type: 'dragCancel', x: 0, y: 0, data: { items: [], dragOperationsMask: 1 } });
  else if (kind === 'key') await key(rest[0]);
  else if (kind === 'type') await send('Input.insertText', { text: rest.join(' ') });
  else throw new Error(`not a line: ${line}`);
  if (!['drag', 'wait', 'shot', 'js', 'size', 'reload', 'window', 'pace'].includes(kind)) await sleep(pace);
}
socket.close();
