import fs from 'node:fs';
import path from 'node:path';

const repoRoot = process.cwd();
const targets = [
  'src/components/Collections.tsx',
  'src/components/settings/SubscriptionsPanel.tsx',
  'src/components/settings/ui.tsx',
  'src/components/video/VideoPlayer.tsx',
];

const violations = [];

for (const relativePath of targets) {
  const absolutePath = path.join(repoRoot, relativePath);
  const source = fs.readFileSync(absolutePath, 'utf8');
  const matches = source.match(/style=\{\{/g);
  if (matches && matches.length > 0) {
    violations.push(`${relativePath}: found ${matches.length} inline style object literal(s)`);
  }
}

if (violations.length > 0) {
  console.error('Inline style guard failed for hot UI files:');
  for (const violation of violations) {
    console.error(`- ${violation}`);
  }
  process.exit(1);
}

console.log('Hot UI inline style guard passed.');
