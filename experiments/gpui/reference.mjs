#!/usr/bin/env node
// Plays a script into the Electron app over the Chrome DevTools protocol and
// saves pictures, taking the same lines as the GPUI off-screen test
// (`src/offscreen.rs`), so one script photographs a view in both apps:
//
//   move X Y | down X Y [clicks] | up X Y [clicks] | rdown X Y | rup X Y
//   scroll X Y DISTANCE | key <keystroke> | type <text> | wait <ms>
//   shot <name> | size W H | js <expression> | reload
//
// `size` sets the viewport in points (pictures are twice that), `js` prints
// an expression's value, and `reload` loads the window again so that a
// script starts where a freshly opened app does (the GPUI test opens the
// app anew each run and ignores the line). The app must be a development build started with
// its automation switches on a disposable library copy; see the README.
//
//   node reference.mjs steps.txt outdir            (PICTO_REFERENCE_PORT, default 9222)
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';

const [script, out] = process.argv.slice(2);
mkdirSync(out, { recursive: true });
const port = process.env.PICTO_REFERENCE_PORT || '9222';
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
// The main window is the page served from the renderer's own address.
const page = targets.find((target) => target.type === 'page' && /^http:\/\/(127\.0\.0\.1|localhost)/.test(target.url));
if (!page) throw new Error('reference app not found');
const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.onopen = resolve;
  socket.onerror = reject;
});
let next = 0;
const pending = new Map();
socket.onmessage = (event) => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    const { resolve, reject } = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) reject(new Error(JSON.stringify(message.error)));
    else resolve(message.result);
  }
};
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = ++next;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
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
};
async function key(stroke) {
  const parts = stroke.split('-');
  const name = parts.pop();
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
  const base = { key: keyName, code, windowsVirtualKeyCode: keyCode, nativeVirtualKeyCode: keyCode, modifiers };
  // With Command or Control held the key is a shortcut, not text.
  const typed = modifiers & 6 ? undefined : text;
  await send('Input.dispatchKeyEvent', { type: typed ? 'keyDown' : 'rawKeyDown', ...base, text: typed });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
}

for (const raw of readFileSync(script, 'utf8').split('\n')) {
  const line = raw.trim();
  if (!line || line.startsWith('#')) continue;
  const [kind, ...rest] = line.split(' ');
  const numbers = rest.map(Number);
  if (kind === 'wait') await sleep(numbers[0]);
  else if (kind === 'shot') {
    const { data } = await send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(path.join(out, `${rest[0]}.png`), Buffer.from(data, 'base64'));
  } else if (kind === 'size') {
    await send('Emulation.setDeviceMetricsOverride', { width: numbers[0], height: numbers[1], deviceScaleFactor: 2, mobile: false });
    await sleep(500);
  } else if (kind === 'reload') {
    await send('Page.reload');
    await sleep(4000);
  } else if (kind === 'js') {
    const result = await send('Runtime.evaluate', { expression: rest.join(' '), returnByValue: true, awaitPromise: true });
    console.log(JSON.stringify(result.result?.value ?? result.exceptionDetails?.text ?? null));
  } else if (kind === 'move') await mouse('mouseMoved', numbers[0], numbers[1]);
  else if (kind === 'down') await mouse('mousePressed', numbers[0], numbers[1], 'left', numbers[2] || 1);
  else if (kind === 'up') await mouse('mouseReleased', numbers[0], numbers[1], 'left', numbers[2] || 1);
  else if (kind === 'rdown') await mouse('mousePressed', numbers[0], numbers[1], 'right', 1);
  else if (kind === 'rup') await mouse('mouseReleased', numbers[0], numbers[1], 'right', 1);
  else if (kind === 'scroll')
    await mouse('mouseWheel', numbers[0], numbers[1], 'none', 0, { deltaX: 0, deltaY: -numbers[2] });
  else if (kind === 'key') await key(rest[0]);
  else if (kind === 'type') await send('Input.insertText', { text: rest.join(' ') });
  else throw new Error(`not a line: ${line}`);
  if (!['wait', 'shot', 'js', 'size', 'reload'].includes(kind)) await sleep(200);
}
socket.close();
