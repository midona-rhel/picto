import { describe, expect, it, vi } from 'vitest';
import { isEditableTarget } from './editableTarget';
import { buildPanelVisibilityContextEntries, titlebarLayoutWidth } from './AppShell';

describe('isEditableTarget', () => {
  it('recognizes editable controls and contenteditable elements', () => {
    expect(isEditableTarget(document.createElement('input'))).toBe(true);
    expect(isEditableTarget(document.createElement('textarea'))).toBe(true);
    expect(isEditableTarget(document.createElement('select'))).toBe(true);

    const editable = document.createElement('div');
    editable.setAttribute('contenteditable', 'true');
    expect(isEditableTarget(editable)).toBe(true);

    const child = document.createElement('span');
    editable.append(child);
    expect(isEditableTarget(child)).toBe(true);
  });

  it('leaves non-editable targets eligible for app shortcuts', () => {
    expect(isEditableTarget(document.createElement('button'))).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
  });
});

describe('panel visibility context menu', () => {
  it('uses shared panel actions and keeps the menu open for successive toggles', () => {
    const toggleAll = vi.fn();
    const toggleSidebar = vi.fn();
    const toggleInspector = vi.fn();
    const entries = buildPanelVisibilityContextEntries({ toggleAll, toggleSidebar, toggleInspector });

    expect(entries.map((entry) => 'label' in entry ? entry.label : null)).toEqual([
      'Toggle All Panels',
      'Toggle Sidebar',
      'Toggle Inspector',
    ]);
    for (const entry of entries) {
      expect('keepOpen' in entry && entry.keepOpen).toBe(true);
    }
    if ('action' in entries[1]) entries[1].action();
    expect(toggleSidebar).toHaveBeenCalledOnce();
  });
});

describe('settled titlebar geometry', () => {
  it('reserves the inspector without allowing a negative main titlebar width', () => {
    expect(titlebarLayoutWidth(1_440, 360)).toBe(1_080);
    expect(titlebarLayoutWidth(320, 480)).toBe(0);
  });
});
