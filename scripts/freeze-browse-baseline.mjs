import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFile, mkdir, readFile, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';

// A reproducible working-tree baseline, including uncommitted source. Never reset
// the checkout or include another task's modeling work / generated artifacts.
const root = process.cwd();
const output = path.resolve(process.argv[2] ?? `artifacts/benchmarks/browse/${new Date().toISOString().replaceAll(':', '-')}/baseline`);
await mkdir(output, { recursive: true });
try { await stat(path.join(output, 'source-manifest.json')); throw new Error('Baseline already exists'); }
catch (error) { if (error.code !== 'ENOENT') throw error; }
const git = args => execFileSync('git', args, { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
const paths = [...new Set(git(['ls-files', '-z', '--cached', '--others', '--exclude-standard']).split('\0').filter(Boolean))]
  .filter(file => !/^(modeling|artifacts|\.tmp|target|node_modules)\//.test(file)).sort();
const files = [];
for (const file of paths) {
  const source = path.join(root, file);
  let bytes;
  try { bytes = await readFile(source); }
  catch (error) { if (error.code === 'ENOENT') { files.push({ path: file, deleted: true }); continue; } throw error; }
  const destination = path.join(output, 'source', file);
  await mkdir(path.dirname(destination), { recursive: true });
  await copyFile(source, destination);
  files.push({ path: file, bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') });
}
const sourceHash = createHash('sha256').update(JSON.stringify(files)).digest('hex');
await writeFile(path.join(output, 'source-manifest.json'), JSON.stringify({
  captured_at: new Date().toISOString(), sourceHash, commit: git(['rev-parse', 'HEAD']).trim(),
  status: git(['status', '--porcelain']), node: process.version,
  rust: execFileSync('rustc', ['--version'], { encoding: 'utf8' }).trim(),
  build_environment: Object.fromEntries([
    'RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','CARGO_BUILD_TARGET','CARGO_INCREMENTAL',
    'CARGO_PROFILE_RELEASE_OPT_LEVEL','CARGO_PROFILE_RELEASE_LTO',
    'CARGO_PROFILE_RELEASE_CODEGEN_UNITS','CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS',
  ].map(key => [key,process.env[key] ?? null])),
  profile: 'release', platform: process.platform, architecture: process.arch, files,
}, null, 2), { flag: 'wx' });
console.log(JSON.stringify({ output, sourceHash, files: files.length }));
