import { describe, expect, it } from 'vitest';
import { parseReleaseNotes } from './UpdateModal';

describe('release-note parsing', () => {
  it('keeps wrapped bullet and paragraph lines in the same aligned block', () => {
    expect(parseReleaseNotes(`# Picto 0.6.3-alpha

An update with a deliberately wrapped
introduction.

## Media

- Keeps the thumbnail visible while the full image
  is loading.
- Fixes navigation.`)).toEqual([
      { kind: 'heading', level: 1, text: 'Picto 0.6.3-alpha' },
      { kind: 'paragraph', text: 'An update with a deliberately wrapped introduction.' },
      { kind: 'heading', level: 2, text: 'Media' },
      { kind: 'bullet', text: 'Keeps the thumbnail visible while the full image is loading.' },
      { kind: 'bullet', text: 'Fixes navigation.' },
    ]);
  });
});
