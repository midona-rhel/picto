import { copyFileSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const targetRoot = process.env.CARGO_TARGET_DIR
  ? path.resolve(root, process.env.CARGO_TARGET_DIR)
  : path.join(root, 'target');
const releaseDirectory = path.join(targetRoot, 'release');
const destination = path.join(root, 'native', 'picto-node');
const isRuntimeLibrary = (name) =>
  name.endsWith('.dll') || name.endsWith('.dylib') || /\.so(?:\.|$)/.test(name);

mkdirSync(destination, { recursive: true });
for (const name of readdirSync(destination)) {
  if (isRuntimeLibrary(name)) rmSync(path.join(destination, name));
}

const libraries = readdirSync(releaseDirectory).filter(isRuntimeLibrary);
for (const name of libraries) {
  copyFileSync(path.join(releaseDirectory, name), path.join(destination, name));
}

console.log(
  libraries.length > 0
    ? `Staged native runtime libraries: ${libraries.join(', ')}`
    : 'No separate native runtime libraries to stage.',
);
