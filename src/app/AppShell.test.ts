import { describe, expect, it } from 'vitest';
import { isEditableTarget } from './editableTarget';

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
