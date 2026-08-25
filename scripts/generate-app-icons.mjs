#!/usr/bin/env node

import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Resvg } from '@resvg/resvg-js';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const build = path.join(root, 'build');
const sources = path.join(build, 'icons');

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

function createIcns(images) {
  const chunks = images.map(({ type, png }) => {
    const chunk = Buffer.alloc(8 + png.length);
    chunk.write(type, 0, 4, 'ascii');
    chunk.writeUInt32BE(chunk.length, 4);
    png.copy(chunk, 8);
    return chunk;
  });
  const header = Buffer.alloc(8);
  header.write('icns', 0, 4, 'ascii');
  header.writeUInt32BE(8 + chunks.reduce((total, chunk) => total + chunk.length, 0), 4);
  return Buffer.concat([header, ...chunks]);
}

async function main() {
  await mkdir(build, { recursive: true });
  const flatSource = path.join(sources, 'picto-flat.svg');
  const macSource = path.join(sources, 'picto-macos.svg');
  const [flat512, mac1024] = await Promise.all([render(flatSource, 512), render(macSource, 1024)]);
  await Promise.all([
    writeFile(path.join(build, 'icon-flat.png'), flat512),
    writeFile(path.join(build, 'icon-macos.png'), mac1024),
  ]);

  const icoSizes = [16, 24, 32, 48, 64, 128, 256];
  const icoImages = await Promise.all(icoSizes.map(async (size) => ({ size, png: await render(flatSource, size) })));
  await writeFile(path.join(build, 'icon.ico'), createIco(icoImages));

  const icnsTypes = new Map([
    [16, 'icp4'], [32, 'icp5'], [64, 'icp6'], [128, 'ic07'],
    [256, 'ic08'], [512, 'ic09'], [1024, 'ic10'],
  ]);
  const icnsImages = await Promise.all([...icnsTypes].map(async ([size, type]) => ({
    type,
    png: await render(macSource, size),
  })));
  await writeFile(path.join(build, 'icon.icns'), createIcns(icnsImages));

  console.log('Generated Picto application icons.');
}

await main();
