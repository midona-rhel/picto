import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);

let binding;
try {
  // eslint-disable-next-line import/no-unresolved, global-require
  binding = require('../native/picto-node/index.node');
} catch (error) {
  throw new Error(
    `Failed to load native addon 'index.node'. Build native/picto-node first. Original error: ${String(error)}`,
  );
}

export async function initialize(libraryPath) {
  return binding.initialize(libraryPath);
}

export async function openLibrary(libraryPath) {
  return binding.openLibrary(libraryPath);
}

export async function closeLibrary() {
  return binding.closeLibrary();
}

export async function invoke(command, args = {}) {
  const resultJson = await binding.invoke(command, JSON.stringify(args ?? {}));
  if (resultJson == null || resultJson === 'null' || resultJson === '') return null;
  return JSON.parse(resultJson);
}

export function onNativeEvent(handler) {
  if (typeof binding.registerEventCallback !== 'function') {
    throw new Error('Native addon missing registerEventCallback');
  }
  return binding.registerEventCallback((name, payloadJson) => {
    let payload = null;
    try {
      payload = payloadJson ? JSON.parse(payloadJson) : null;
    } catch {
      payload = payloadJson;
    }
    handler(name, payload);
  });
}
