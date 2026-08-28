import { describe, expect, it } from 'vitest';
import { getFieldDef, getFieldOptions } from './fieldConfig';

describe('smart-folder field contract', () => {
  it('keeps only the supported text and date-created behavior', () => {
    expect(getFieldOptions().map((field) => field.value)).toContain('date_created');
    expect(getFieldOptions().map((field) => field.value)).not.toContain('shape');
    expect(getFieldDef('name').operators.map((operator) => operator.value)).toEqual(['contains']);
    expect(getFieldDef('notes').operators.map((operator) => operator.value)).toEqual([
      'contains',
      'is_empty',
      'is_not_empty',
    ]);
  });
});
