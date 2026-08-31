import { describe, expect, it } from 'vitest';
import { associatedFilesFromArguments, classifyAssociatedFile } from './associatedFileService.mjs';

describe('associated file routing', () => {
  it('recognizes Picto Packs and library packages case-insensitively', () => {
    expect(classifyAssociatedFile('/tmp/Portfolio.PICTO-PACK')).toEqual({
      kind: 'picto-pack',
      path: '/tmp/Portfolio.PICTO-PACK',
    });
    expect(classifyAssociatedFile('/tmp/Main.LIBRARY')).toEqual({
      kind: 'library',
      path: '/tmp/Main.LIBRARY',
    });
  });

  it('resolves relative second-instance arguments and ignores unrelated flags', () => {
    expect(associatedFilesFromArguments([
      '--inspect',
      'Portfolio.picto-pack',
      'Portfolio.picto-pack',
      'photo.jpg',
    ], '/tmp')).toEqual([{ kind: 'picto-pack', path: '/tmp/Portfolio.picto-pack' }]);
  });

  it('accepts encoded file URLs from desktop launchers', () => {
    expect(classifyAssociatedFile('file:///tmp/My%20Library.library')).toEqual({
      kind: 'library',
      path: '/tmp/My Library.library',
    });
  });
});

