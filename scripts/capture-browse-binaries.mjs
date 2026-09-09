import { copyFile, mkdir, readFile, writeFile, access } from 'node:fs/promises';
import { constants } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';

const output = path.resolve(process.argv[2]);
try { await access(path.join(output, 'binary-manifest.json')); throw new Error('Binary baseline already exists'); }
catch (error) { if (error.code !== 'ENOENT') throw error; }
const baseline = JSON.parse(await readFile(path.join(output, 'source-manifest.json'), 'utf8'));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
for (const entry of baseline.files.filter(entry => /(?:\.rs|Cargo\.(?:toml|lock))$/.test(entry.path) && !entry.deleted)) {
  if (hash(await readFile(entry.path)) !== entry.sha256) throw new Error(`Backend source changed before baseline capture: ${entry.path}`);
}
const suffix = process.platform === 'win32' ? '.exe' : '';
const names = ['grid_query_scale_probe', 'grid_window_scale_probe', 'scale_probe',
  'smart_folder_scale_probe', 'cold_search_window_probe', 'cold_read_profile'];
const files = names.map(name => path.join('target/release/examples', name + suffix));
const log = await readFile(path.join(output, 'suite/fts-200000.log'), 'utf8');
const test = log.match(/Running[^\n]*\(([^)]+fts_scale[^)]+)\)/)?.[1];
if (test) files.push(test);
const manifest = [];
await mkdir(path.join(output, 'binaries'), { recursive: true });
for (const file of files) {
  const destination = path.join(output, 'binaries', path.basename(file));
  await copyFile(file, destination, constants.COPYFILE_EXCL);
  manifest.push({ name: path.basename(file), sha256: hash(await readFile(destination)) });
}
await writeFile(path.join(output, 'binary-manifest.json'), JSON.stringify({ sourceHash: baseline.sourceHash, files: manifest }, null, 2), { flag: 'wx' });
console.log(JSON.stringify(manifest));
