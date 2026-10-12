import { describe, expect, it } from 'vitest';
import { permanentDeleteMessage } from './permanentDeleteRequest';

describe('permanentDeleteMessage', () => {
  it('says a delete within the limit can be undone until Picto closes', () => {
    expect(permanentDeleteMessage(3, 500)).toBe('You can undo this until Picto closes. After that the files are removed.');
  });

  it('warns that a delete past the limit cannot be undone', () => {
    expect(permanentDeleteMessage(501, 500)).toBe('This is more than the 500 items Picto keeps for undo, so it cannot be undone.');
  });

  it('states the limit when the count is not known', () => {
    expect(permanentDeleteMessage(null, 500)).toContain('500');
  });
});
