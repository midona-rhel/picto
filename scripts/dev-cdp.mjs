#!/usr/bin/env node
/**
 * Dev CDP driver — screenshot and JS eval against the running dev app.
 * Requires the dev stack (remote-debugging-port 9222 is enabled in dev).
 *
 * Usage:
 *   node scripts/dev-cdp.mjs shot /tmp/picto.png
 *   node scripts/dev-cdp.mjs eval "document.title"
 */
import { writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const WebSocket = require('ws');

const [, , command, arg] = process.argv;

async function getPageTarget() {
  const response = await fetch('http://127.0.0.1:9222/json');
  const targets = await response.json();
  const pages = targets.filter((t) => t.type === 'page' && t.title !== 'DevTools');
  const requestedTitle = process.env.PICTO_CDP_TARGET;
  const requestedPage = requestedTitle ? pages.find((t) => t.title === requestedTitle) : null;
  if (requestedTitle && !requestedPage) {
    throw new Error(`No CDP page named "${requestedTitle}"; found: ${pages.map((page) => page.title).join(', ')}`);
  }
  const page = requestedPage ?? pages.find((t) => t.title === 'Picto') ?? pages[0];
  if (!page) throw new Error('No app page target found');
  return page;
}

function cdp(ws) {
  let seq = 0;
  const pending = new Map();
  ws.on('message', (data) => {
    const msg = JSON.parse(data.toString());
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) reject(new Error(msg.error.message));
      else resolve(msg.result);
    }
  });
  return (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++seq;
      pending.set(id, { resolve, reject });
      ws.send(JSON.stringify({ id, method, params }));
    });
}

async function main() {
  const target = await getPageTarget();
  const ws = new WebSocket(target.webSocketDebuggerUrl, { maxPayload: 64 * 1024 * 1024 });
  await new Promise((resolve, reject) => {
    ws.on('open', resolve);
    ws.on('error', reject);
  });
  const send = cdp(ws);

  if (command === 'shot') {
    const out = arg || '/tmp/picto-cdp.png';
    const { data } = await send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(out, Buffer.from(data, 'base64'));
    console.log(out);
  } else if (command === 'eval') {
    const { result, exceptionDetails } = await send('Runtime.evaluate', {
      expression: arg,
      returnByValue: true,
      awaitPromise: true,
    });
    if (exceptionDetails) {
      console.error(exceptionDetails.exception?.description ?? 'evaluation failed');
      process.exitCode = 1;
    } else {
      console.log(JSON.stringify(result.value));
    }
  } else {
    console.error('Usage: dev-cdp.mjs shot [path] | eval "<js>"');
    process.exitCode = 1;
  }
  ws.close();
}

main().catch((err) => {
  console.error(err.message);
  process.exit(1);
});
