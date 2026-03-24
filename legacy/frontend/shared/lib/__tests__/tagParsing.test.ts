import { describe, expect, it } from 'vitest';
import { extractNamespace, parseTagString } from '../tagParsing';

describe('tagParsing', () => {
  it('preserves any syntactically valid namespace prefix', () => {
    expect(parseTagString('lore:dragon')).toEqual({
      namespace: 'lore',
      subtag: 'dragon',
    });
  });

  it('treats invalid namespace candidates as literal tag text', () => {
    expect(parseTagString('>:(:mood')).toEqual({
      namespace: '',
      subtag: '>:(:mood',
    });
  });

  it('extracts namespace using the same generic parsing rules', () => {
    expect(extractNamespace('character:saber')).toBe('character');
    expect(extractNamespace('lore:holy-grail')).toBe('lore');
  });
});
