#!/usr/bin/env node

// Draws the icons in `build/` from the SVG files in `build/icons/`: the
// app's icon for Windows and Linux (`icon.ico`, `icon-flat.png`), the icons
// of `.library` folders and `.picto-pack` files (`.ico`, `.png` and, on
// macOS, `.icns`), and the Icon Composer file `Picto.icon` with the picture
// of its one layer. `build/icons/README.md` describes the artwork.
//
// The package script calls this on macOS before it compiles `Picto.icon`,
// because the layer's picture is not tracked.

import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
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

// Returns the SVG file `source` drawn as a PNG that is `size` pixels wide.
async function render(source, size) {
  const svg = await readFile(source, 'utf8');
  return new Resvg(svg, { fitTo: { mode: 'width', value: size } }).render().asPng();
}

// Returns the bytes of an `.ico` file that holds the given PNG pictures,
// each with its width in pixels.
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

// Writes the `.icns` file `output` from the SVG file `source` with macOS's
// `iconutil`. The file holds the picture at 16, 32, 128, 256 and 512 points,
// each at one and at two pixels per point.
async function writeIcns(source, output) {
  const work = await mkdtemp(path.join(tmpdir(), 'picto-icns-'));
  try {
    const iconset = path.join(work, 'icon.iconset');
    await mkdir(iconset);
    for (const points of [16, 32, 128, 256, 512]) {
      await writeFile(path.join(iconset, `icon_${points}x${points}.png`), await render(source, points));
      await writeFile(path.join(iconset, `icon_${points}x${points}@2x.png`), await render(source, points * 2));
    }
    await execFileAsync('iconutil', ['--convert', 'icns', '--output', output, iconset]);
  } finally {
    await rm(work, { recursive: true, force: true });
  }
}

// The whole book is one layer, so that Icon Composer lights and shades all
// of its strokes together, as one shape.
const composerLayers = [
  ['01-book', 'picto-macos-book.svg'],
];

// Icon Composer's tile fills the canvas of 1024 px, while `picto-flat.svg`
// draws its tile 64 px inside the canvas (896 px wide). Scaling the book by
// 1024/896 keeps it the same size relative to the tile on every system.
const BOOK_SCALE = 1024 / 896;

function composerGroup(name) {
  return {
    'blur-material': null,
    layers: [{
      glass: true,
      hidden: false,
      'image-name': `${name}.png`,
      name,
      position: {
        scale: BOOK_SCALE,
        'translation-in-points': [0, 0],
      },
    }],
    lighting: 'combined',
    // The book has no drop shadow, so that it does not look raised above
    // the tile.
    shadow: { kind: 'none', opacity: 0 },
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
    await writeIcns(librarySource, path.join(build, 'library.icns'));
    await writeIcns(packSource, path.join(build, 'picto-pack.icns'));
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
