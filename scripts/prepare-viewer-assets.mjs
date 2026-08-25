import { copyFile, mkdir, readFile, readdir, unlink, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = resolve(root, 'node_modules/@ruffle-rs/ruffle');
const destination = resolve(root, 'public/vendor/ruffle');
const rootManifest = JSON.parse(await readFile(resolve(root, 'package.json'), 'utf8'));
const packageManifest = JSON.parse(await readFile(resolve(source, 'package.json'), 'utf8'));
if (rootManifest.dependencies?.['@ruffle-rs/ruffle'] !== packageManifest.version) {
  throw new Error(`Ruffle must be pinned exactly; expected ${rootManifest.dependencies?.['@ruffle-rs/ruffle']}, installed ${packageManifest.version}.`);
}
const assets = (await readdir(source)).filter((name) => name.endsWith('.js') || name.endsWith('.wasm'));

await mkdir(destination, { recursive: true });
for (const name of await readdir(destination)) {
  if ((name.endsWith('.js') || name.endsWith('.wasm')) && !assets.includes(name)) {
    await unlink(resolve(destination, name));
  }
}
for (const name of assets) {
  await copyFile(resolve(source, name), resolve(destination, name));
}
await writeFile(resolve(destination, 'picto-runtime.json'), `${JSON.stringify({
  package: '@ruffle-rs/ruffle',
  version: packageManifest.version,
}, null, 2)}\n`);
