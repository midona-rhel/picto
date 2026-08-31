#!/usr/bin/env node

import { execFile } from 'node:child_process';
import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { Resvg } from '@resvg/resvg-js';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const build = path.join(root, 'build');
const sources = path.join(build, 'icons');
const iconComposer = path.join(build, 'Picto.icon');
const iconComposerAssets = path.join(iconComposer, 'Assets');
const execFileAsync = promisify(execFile);

async function render(source, size) {
  const svg = await readFile(source, 'utf8');
  return new Resvg(svg, { fitTo: { mode: 'width', value: size } }).render().asPng();
}

function createIco(images) {
  const header = Buffer.alloc(6 + images.length * 16);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(images.length, 4);
  let offset = header.length;
  images.forEach(({ size, png }, index) => {
    const entry = 6 + index * 16;
    header[entry] = size === 256 ? 0 : size;
    header[entry + 1] = size === 256 ? 0 : size;
    header[entry + 2] = 0;
    header[entry + 3] = 0;
    header.writeUInt16LE(1, entry + 4);
    header.writeUInt16LE(32, entry + 6);
    header.writeUInt32LE(png.length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += png.length;
  });
  return Buffer.concat([header, ...images.map(({ png }) => png)]);
}

const composerLayers = [
  ['01-spine', 'picto-macos-01-spine.svg'],
  ['02-under-pages', 'picto-macos-02-under-pages.svg'],
  ['03-open-book', 'picto-macos-03-open-book.svg'],
];

function composerGroup(name) {
  return {
    'blur-material': null,
    layers: [{
      glass: true,
      hidden: false,
      'image-name': `${name}.png`,
      name,
      position: {
        scale: 1,
        'translation-in-points': [0, 0],
      },
    }],
    lighting: 'individual',
    shadow: { kind: 'neutral', opacity: 0.28 },
    specular: true,
    translucency: { enabled: false, value: 0 },
  };
}

async function main() {
  await Promise.all([
    mkdir(build, { recursive: true }),
    mkdir(iconComposerAssets, { recursive: true }),
  ]);
  const flatSource = path.join(sources, 'picto-flat.svg');
  const flat512 = await render(flatSource, 512);
  await writeFile(path.join(build, 'icon-flat.png'), flat512);

  const icoSizes = [16, 24, 32, 48, 64, 128, 256];
  const icoImages = await Promise.all(icoSizes.map(async (size) => ({ size, png: await render(flatSource, size) })));
  await writeFile(path.join(build, 'icon.ico'), createIco(icoImages));

  const librarySource = path.join(sources, 'picto-library-folder.svg');
  const libraryPng = path.join(build, 'library-folder.png');
  await writeFile(libraryPng, await render(librarySource, 1024));
  const libraryIcoImages = await Promise.all(icoSizes.map(async (size) => ({ size, png: await render(librarySource, size) })));
  await writeFile(path.join(build, 'library.ico'), createIco(libraryIcoImages));

  const packSource = path.join(sources, 'picto-pack.svg');
  const packPng = path.join(build, 'picto-pack.png');
  await writeFile(packPng, await render(packSource, 1024));
  const packIcoImages = await Promise.all(icoSizes.map(async (size) => ({ size, png: await render(packSource, size) })));
  await writeFile(path.join(build, 'picto-pack.ico'), createIco(packIcoImages));

  if (process.platform === 'darwin') {
    const appBuilderArch = process.arch === 'arm64' ? 'arm64' : 'amd64';
    const appBuilder = path.join(root, 'node_modules', 'app-builder-bin', 'mac', `app-builder_${appBuilderArch}`);
    const output = path.join(build, 'library-icns-output');
    await rm(output, { recursive: true, force: true });
    await mkdir(output, { recursive: true });
    await execFileAsync(appBuilder, ['icon', '--format=icns', `--out=${output}`, `--input=${libraryPng}`, `--root=${root}`]);
    await rename(path.join(output, 'icon.icns'), path.join(build, 'library.icns'));
    await rm(output, { recursive: true, force: true });

    const packOutput = path.join(build, 'picto-pack-icns-output');
    await rm(packOutput, { recursive: true, force: true });
    await mkdir(packOutput, { recursive: true });
    await execFileAsync(appBuilder, ['icon', '--format=icns', `--out=${packOutput}`, `--input=${packPng}`, `--root=${root}`]);
    await rename(path.join(packOutput, 'icon.icns'), path.join(build, 'picto-pack.icns'));
    await rm(packOutput, { recursive: true, force: true });
  }

  await Promise.all(composerLayers.map(async ([name, source]) => {
    const png = await render(path.join(sources, source), 1024);
    await writeFile(path.join(iconComposerAssets, `${name}.png`), png);
  }));
  await writeFile(path.join(iconComposer, 'icon.json'), `${JSON.stringify({
    fill: { solid: 'srgb:0.83529,0.84314,0.86275,1.00000' },
    groups: composerLayers.map(([name]) => composerGroup(name)),
    'supported-platforms': {
      circles: [],
      squares: 'shared',
    },
  }, null, 2)}\n`);

  console.log('Generated Picto application, library, and Picto Pack icons.');
}

await main();
