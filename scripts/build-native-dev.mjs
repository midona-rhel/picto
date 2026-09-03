#!/usr/bin/env node

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const nativeRoot = path.join(root, 'native', 'picto-node');
const napi = path.join(nativeRoot, 'node_modules', '@napi-rs', 'cli', 'scripts', 'index.js');
const declarationPath = path.join(nativeRoot, 'index.d.ts');
const existingDeclaration = existsSync(declarationPath) ? readFileSync(declarationPath) : null;
const buildEnvironment = { ...process.env };
const pathKey = Object.keys(buildEnvironment).find((key) => key.toLowerCase() === 'path') ?? 'PATH';
const prependToolDirectory = (executable) => {
  if (!existsSync(executable)) return;
  buildEnvironment[pathKey] = `${path.dirname(executable)}${path.delimiter}${buildEnvironment[pathKey] ?? ''}`;
};

// The Rust cmake crate can infer a newer Visual Studio generator from an
// installed preview toolchain than the installed CMake version understands.
// Pin the generator to CMake's own Windows default unless the user selected one.
if (process.platform === 'win32' && !buildEnvironment.CMAKE_GENERATOR) {
  const cmakeHelp = spawnSync('cmake', ['--help'], { encoding: 'utf8' });
  const defaultGenerator = cmakeHelp.stdout?.match(/^\* (Visual Studio \d+ \d+)/m)?.[1];
  if (defaultGenerator) buildEnvironment.CMAKE_GENERATOR = defaultGenerator;
}
if (process.platform === 'win32') {
  if (buildEnvironment.USERPROFILE) {
    prependToolDirectory(path.join(buildEnvironment.USERPROFILE, '.cargo', 'bin', 'cargo.exe'));
  }
  if (buildEnvironment.LOCALAPPDATA) {
    prependToolDirectory(path.join(buildEnvironment.LOCALAPPDATA, 'Programs', 'NASM', 'nasm.exe'));
  }
}
const child = spawn(process.execPath, [napi, 'build'], {
  cwd: nativeRoot,
  env: buildEnvironment,
  stdio: 'inherit',
});

child.once('error', (error) => {
  console.error('[native-watch] failed to start native build:', error);
  process.exitCode = 1;
});
child.once('exit', (code, signal) => {
  // A hot-reload build only needs the binary. Preserve the checked-in
  // declaration byte-for-byte so napi's platform line endings do not dirty it.
  if (existingDeclaration) writeFileSync(declarationPath, existingDeclaration);
  process.exitCode = code ?? (signal ? 1 : 0);
});
