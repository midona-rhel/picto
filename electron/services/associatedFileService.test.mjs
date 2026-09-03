import { describe, expect, it } from 'vitest';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { associatedFilesFromArguments, classifyAssociatedFile } from './associatedFileService.mjs';

describe('associated file routing', () => {
  it('recognizes Picto Packs and library packages case-insensitively', () => {
    const pack = path.resolve('/tmp/Portfolio.PICTO-PACK');
    const library = path.resolve('/tmp/Main.LIBRARY');
    expect(classifyAssociatedFile(pack)).toEqual({
      kind: 'picto-pack',
      path: pack,
    });
    expect(classifyAssociatedFile(library)).toEqual({
      kind: 'library',
      path: library,
    });
  });

  it('resolves relative second-instance arguments and ignores unrelated flags', () => {
    expect(associatedFilesFromArguments([
      '--inspect',
      'Portfolio.picto-pack',
      'Portfolio.picto-pack',
      'photo.jpg',
    ], path.resolve('/tmp'))).toEqual([{
      kind: 'picto-pack',
      path: path.resolve('/tmp/Portfolio.picto-pack'),
    }]);
  });

  it('accepts encoded file URLs from desktop launchers', () => {
    const library = path.resolve('/tmp/My Library.library');
    expect(classifyAssociatedFile(pathToFileURL(library).href)).toEqual({
      kind: 'library',
      path: library,
    });
  });
});

