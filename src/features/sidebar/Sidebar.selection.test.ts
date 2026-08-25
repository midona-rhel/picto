import { describe, expect, it } from 'vitest';
import { nextSidebarSelection } from './Sidebar';

describe('sidebar folder selection', () => {
  it('keeps the plainly clicked folder as the selection anchor', () => {
    expect([...nextSidebarSelection(new Set(), 'folder:1', 'replace')]).toEqual(['folder:1']);
  });

  it('extends and toggles a multi-selection', () => {
    const anchored = new Set(['folder:1']);
    expect([...nextSidebarSelection(anchored, 'folder:2', 'toggle')]).toEqual(['folder:1', 'folder:2']);
    expect([...nextSidebarSelection(new Set(['folder:1', 'folder:2']), 'folder:1', 'toggle')]).toEqual(['folder:2']);
  });

  it('adds every visible folder in a shift range', () => {
    const selection = nextSidebarSelection(
      new Set(['folder:1']),
      'folder:4',
      'range',
      ['folder:1', 'folder:2', 'folder:3', 'folder:4'],
    );
    expect([...selection]).toEqual(['folder:1', 'folder:2', 'folder:3', 'folder:4']);
  });
});
