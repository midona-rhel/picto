import { describe, expect, it } from 'vitest';
import { deriveActiveGridScopeCount, deriveGridScopeKey } from '../scopeModel';

describe('deriveGridScopeKey', () => {
  it('returns null outside the images view', () => {
    expect(deriveGridScopeKey({
      currentView: 'subscriptions',
      activeFolderId: 1,
      activeCollectionId: 2,
      activeSmartFolderId: '3',
      activeStatusFilter: 'inbox',
    })).toBeNull();
  });

  it('prefers collection over folder and smart-folder scope', () => {
    expect(deriveGridScopeKey({
      currentView: 'images',
      activeFolderId: 1,
      activeCollectionId: 2,
      activeSmartFolderId: '3',
      activeStatusFilter: 'inbox',
    })).toBe('collection:2');
  });

  it('returns folder scope when no collection is active', () => {
    expect(deriveGridScopeKey({
      currentView: 'images',
      activeFolderId: 7,
      activeCollectionId: null,
      activeSmartFolderId: null,
      activeStatusFilter: null,
    })).toBe('folder:7');
  });

  it('returns smart scope when no collection or folder is active', () => {
    expect(deriveGridScopeKey({
      currentView: 'images',
      activeFolderId: null,
      activeCollectionId: null,
      activeSmartFolderId: 'abc',
      activeStatusFilter: null,
    })).toBe('smart:abc');
  });

  it('returns explicit system scopes before falling back to system:active', () => {
    expect(deriveGridScopeKey({
      currentView: 'images',
      activeFolderId: null,
      activeCollectionId: null,
      activeSmartFolderId: null,
      activeStatusFilter: 'untagged',
    })).toBe('system:untagged');

    expect(deriveGridScopeKey({
      currentView: 'images',
      activeFolderId: null,
      activeCollectionId: null,
      activeSmartFolderId: null,
      activeStatusFilter: null,
    })).toBe('system:active');
  });

  it('treats random as the canonical all-scope with random ordering', () => {
    expect(deriveGridScopeKey({
      currentView: 'images',
      activeFolderId: null,
      activeCollectionId: null,
      activeSmartFolderId: null,
      activeStatusFilter: 'random',
    })).toBe('system:active');
  });
});

describe('deriveActiveGridScopeCount', () => {
  const base = {
    activeFolderId: null,
    activeCollectionId: null,
    activeSmartFolderId: null,
    activeStatusFilter: null,
    activeFolderCount: 12,
    allImagesCount: 100,
    inboxCount: 5,
    uncategorizedCount: 8,
    untaggedCount: 13,
    trashCount: 2,
    smartFolderCounts: { sf1: 21 },
  };

  it('prefers folder count when a folder is active', () => {
    expect(deriveActiveGridScopeCount({
      ...base,
      activeFolderId: 9,
    })).toBe(12);
  });

  it('returns null for collection scope because the count must come from the grid response', () => {
    expect(deriveActiveGridScopeCount({
      ...base,
      activeCollectionId: 4,
    })).toBeNull();
  });

  it('returns smart-folder counts when a smart folder is active', () => {
    expect(deriveActiveGridScopeCount({
      ...base,
      activeSmartFolderId: 'sf1',
    })).toBe(21);
  });

  it('returns the correct system-status counts', () => {
    expect(deriveActiveGridScopeCount({
      ...base,
      activeStatusFilter: 'inbox',
    })).toBe(5);
    expect(deriveActiveGridScopeCount({
      ...base,
      activeStatusFilter: 'uncategorized',
    })).toBe(8);
    expect(deriveActiveGridScopeCount({
      ...base,
      activeStatusFilter: 'untagged',
    })).toBe(13);
    expect(deriveActiveGridScopeCount({
      ...base,
      activeStatusFilter: 'trash',
    })).toBe(2);
  });

  it('falls back to all-images count for the default scope', () => {
    expect(deriveActiveGridScopeCount(base)).toBe(100);
  });
});
