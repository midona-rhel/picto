import { describe, expect, it } from 'vitest';
import type { BaseScope } from '../shared/types/canonical';
import {
  manualImportParamsForScope,
  MEDIA_STATUS_ACTIVE,
  MEDIA_STATUS_INBOX,
} from './filesController';

describe('manual import destination', () => {
  it('imports from Inbox into Inbox', () => {
    expect(manualImportParamsForScope({ kind: 'inbox' })).toEqual({
      initial_status: MEDIA_STATUS_INBOX,
    });
  });

  it('imports from All into the accepted library', () => {
    expect(manualImportParamsForScope({ kind: 'all' })).toEqual({
      initial_status: MEDIA_STATUS_ACTIVE,
    });
  });

  it.each<BaseScope>([
    { kind: 'trash' },
    { kind: 'folder', folder_id: 7 },
    { kind: 'smart_folder', smart_folder_id: 9 },
  ])('keeps %j imports active without inventing scope semantics', (scope) => {
    expect(manualImportParamsForScope(scope).initial_status).toBe(MEDIA_STATUS_ACTIVE);
  });
});
