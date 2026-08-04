import { describe, expect, it } from 'vitest';
import { classifyGridAction } from './gridSettle';

describe('grid runtime settling', () => {
  it('settles system lifecycle changes even when extra scopes are present', () => {
    expect(classifyGridAction(
      {
        status_changed: true,
        extra_grid_scopes: ['smart:7'],
      },
      { kind: 'system', key: 'all' },
      [],
    )).toBe('reconcile_membership');
  });

  it('settles an affected smart folder even when extra scopes omit it', () => {
    expect(classifyGridAction(
      {
        smart_folder_ids: [7],
        extra_grid_scopes: ['system:active'],
      },
      { kind: 'smart_folder', id: 7 },
      [],
    )).toBe('reconcile_membership');
  });

  it('ignores unrelated smart-folder changes', () => {
    expect(classifyGridAction(
      { smart_folder_ids: [8] },
      { kind: 'smart_folder', id: 7 },
      [],
    )).toBe('ignore');
  });
});
