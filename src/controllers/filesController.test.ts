import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import type { BaseScope } from '../shared/types/canonical';
import { chooseAndExportOriginals, manualImportParamsForScope, requestMediaImport } from './filesController';
import * as folderApi from '../platform/folderApi';
import { multiFileImportModalAtom } from '../state/modals';

const closedMultiFileImport = {
  open: false,
  paths: [],
  lifecycle: 'active' as const,
  parentFolderId: null,
  tags: [],
  sourceUrls: [],
  preserveStructure: false,
  deleteAfterIngest: false,
};

afterEach(() => {
  getDefaultStore().set(multiFileImportModalAtom, closedMultiFileImport);
  vi.restoreAllMocks();
});

describe('manual import destination', () => {
  it('imports from Inbox into Inbox', () => {
    expect(manualImportParamsForScope({ kind: 'inbox' })).toEqual({
      lifecycle: 'inbox',
    });
  });

  it('imports from All into the accepted library', () => {
    expect(manualImportParamsForScope({ kind: 'all' })).toEqual({
      lifecycle: 'active',
    });
  });

  it.each<BaseScope>([
    { kind: 'trash' },
    { kind: 'folder', folder_id: 7 },
    { kind: 'smart_folder', smart_folder_id: 9 },
  ])('keeps %j imports active without inventing scope semantics', (scope) => {
    expect(manualImportParamsForScope(scope).lifecycle).toBe('active');
  });
});

describe('multi-file import choice', () => {
  it('asks how an explicit multi-file batch should be represented', () => {
    requestMediaImport(['/tmp/one.png', '/tmp/two.png'], {
      lifecycle: 'inbox',
      parent_folder_id: 7,
      tags: ['artist:test'],
      delete_after_ingest: true,
    });

    expect(getDefaultStore().get(multiFileImportModalAtom)).toEqual({
      open: true,
      paths: ['/tmp/one.png', '/tmp/two.png'],
      lifecycle: 'inbox',
      parentFolderId: 7,
      tags: ['artist:test'],
      sourceUrls: [],
      preserveStructure: false,
      deleteAfterIngest: true,
    });
  });
});

describe('original export picker', () => {
  it('uses the selected directory and preserves original format', async () => {
    const exportMedia = vi.spyOn(folderApi, 'exportMedia').mockResolvedValue({
      selected_item_count: 1,
      selected_media_count: 1,
      exported: 1,
      skipped: 0,
      errors: [],
    });
    (window as any).picto = {
      dialog: { open: vi.fn().mockResolvedValue('/tmp/export') },
    };
    const target = { kind: 'explicit' as const, item_ids: [7] };

    await chooseAndExportOriginals(target);

    expect(exportMedia).toHaveBeenCalledWith(target, {
      output_dir: '/tmp/export',
      format: 'original',
    });
  });
});
