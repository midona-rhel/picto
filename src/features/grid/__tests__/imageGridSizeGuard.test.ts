import fs from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

describe('ImageGrid size guard', () => {
  it('stays under 1200 lines to keep orchestration split across hooks', () => {
    const filePath = path.join(process.cwd(), 'src/features/grid/ImageGrid.tsx');
    const content = fs.readFileSync(filePath, 'utf8');
    const lineCount = content.split(/\r?\n/).length;
    expect(lineCount).toBeLessThanOrEqual(1200);
  });
});
