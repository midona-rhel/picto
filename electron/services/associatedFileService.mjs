import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ASSOCIATED_SUFFIXES = [
  ['.picto-pack', 'picto-pack'],
  ['.library', 'library'],
];

export function classifyAssociatedFile(value, workingDirectory = process.cwd()) {
  if (typeof value !== 'string' || value.length === 0 || value.startsWith('-')) return null;
  let candidate = value;
  if (candidate.startsWith('file://')) {
    try {
      candidate = fileURLToPath(candidate);
    } catch {
      return null;
    }
  }
  const lower = candidate.toLowerCase();
  const match = ASSOCIATED_SUFFIXES.find(([suffix]) => lower.endsWith(suffix));
  if (!match) return null;
  return {
    kind: match[1],
    path: path.isAbsolute(candidate) ? path.normalize(candidate) : path.resolve(workingDirectory, candidate),
  };
}

export function associatedFilesFromArguments(values, workingDirectory = process.cwd()) {
  const seen = new Set();
  const files = [];
  for (const value of values ?? []) {
    const entry = classifyAssociatedFile(value, workingDirectory);
    if (!entry) continue;
    const identity = `${entry.kind}:${entry.path}`;
    if (seen.has(identity)) continue;
    seen.add(identity);
    files.push(entry);
  }
  return files;
}

