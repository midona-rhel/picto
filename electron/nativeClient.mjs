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

export function initRuntime() {
  binding.initRuntime();
}

export async function openLibrary(libraryPath) {
  return binding.openLibrary(libraryPath);
}

export async function closeLibrary() {
  return binding.closeLibrary();
}

export async function invoke(command, args = {}) {
  const resultJson = await invokeSerialized(command, args);
  if (resultJson == null || resultJson === 'null' || resultJson === '') return null;
  return JSON.parse(resultJson);
}

export function invokeSerialized(command, args = {}) {
  return binding.invoke(command, JSON.stringify(args ?? {}));
}

export function startNativeDrag(windowHandle, filePaths, iconRgba, iconWidth, iconHeight) {
  return binding.startNativeDrag(windowHandle, filePaths, iconRgba, iconWidth, iconHeight);
}

export function getAssociatedApplications(filePath) {
  const result = binding.getAssociatedApplications(filePath);
  return JSON.parse(result || '[]');
}

export function openWithApplication(applicationPath, filePath) {
  return binding.openWithApplication(applicationPath, filePath);
}

export function onNativeEvent(handler) {
  if (typeof binding.registerEventCallback !== 'function') {
    throw new Error('Native addon missing registerEventCallback');
  }
  // napi-rs ThreadsafeFunction uses error-first callback convention:
  // (err, ...values) — first arg is null on success, then the actual values.
  return binding.registerEventCallback((_err, name, payloadJson) => {
    let payload = null;
    try {
      payload = payloadJson ? JSON.parse(payloadJson) : null;
    } catch {
      payload = payloadJson;
    }
    handler(name, payload);
  });
}
