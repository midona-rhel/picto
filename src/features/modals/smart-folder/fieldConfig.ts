/**
 * Field definition registry for smart folder predicate rules.
 * Defines available fields, their operator sets, and value input types.
 */

export interface FieldDef {
  key: string;
  label: string;
  operators: { value: string; label: string }[];
  valueType: 'tags' | 'text' | 'number' | 'date' | 'select' | 'filesize' | 'color';
  selectOptions?: { value: string; label: string }[];
  unit?: string;
}

const TAG_OPERATORS = [
  { value: 'include_all', label: 'include all' },
  { value: 'include_any', label: 'include any' },
  { value: 'do_not_include', label: 'do not include' },
];

const NUMERIC_OPERATORS = [
  { value: 'eq', label: 'equals' },
  { value: 'neq', label: 'not equals' },
  { value: 'gt', label: 'greater than' },
  { value: 'gte', label: 'at least' },
  { value: 'lt', label: 'less than' },
  { value: 'lte', label: 'at most' },
  { value: 'between', label: 'between' },
];

const TEXT_OPERATORS = [
  { value: 'contains', label: 'contains' },
  { value: 'is_empty', label: 'is empty' },
  { value: 'is_not_empty', label: 'is not empty' },
];

const DATE_OPERATORS = [
  { value: 'eq', label: 'is' },
  { value: 'gt', label: 'after' },
  { value: 'gte', label: 'on or after' },
  { value: 'lt', label: 'before' },
  { value: 'lte', label: 'on or before' },
  { value: 'between', label: 'between' },
];

export const FIELD_DEFS: FieldDef[] = [
  {
    key: 'tags',
    label: 'Tags',
    operators: TAG_OPERATORS,
    valueType: 'tags',
  },
  {
    key: 'file_type',
    label: 'File Type',
    operators: [
      { value: 'is', label: 'is' },
      { value: 'is_not', label: 'is not' },
    ],
    valueType: 'select',
    selectOptions: [
      { value: 'image', label: 'Image' },
      { value: 'video', label: 'Video' },
      { value: 'audio', label: 'Audio' },
      { value: 'image/png', label: 'PNG' },
      { value: 'image/jpeg', label: 'JPEG' },
      { value: 'video/mp4', label: 'MP4' },
    ],
  },
  {
    key: 'rating',
    label: 'Rating',
    operators: NUMERIC_OPERATORS,
    valueType: 'number',
  },
  {
    key: 'file_size',
    label: 'File Size',
    operators: NUMERIC_OPERATORS,
    valueType: 'filesize',
  },
  {
    key: 'date_added',
    label: 'Date Imported',
    operators: DATE_OPERATORS,
    valueType: 'date',
  },
  {
    key: 'date_created',
    label: 'Date Created',
    operators: DATE_OPERATORS,
    valueType: 'date',
  },
  {
    key: 'date_modified',
    label: 'Date Modified',
    operators: DATE_OPERATORS,
    valueType: 'date',
  },
  {
    key: 'name',
    label: 'Name',
    operators: [{ value: 'contains', label: 'contains' }],
    valueType: 'text',
  },
  {
    key: 'width',
    label: 'Width',
    operators: NUMERIC_OPERATORS,
    valueType: 'number',
    unit: 'px',
  },
  {
    key: 'height',
    label: 'Height',
    operators: NUMERIC_OPERATORS,
    valueType: 'number',
    unit: 'px',
  },
  {
    key: 'duration',
    label: 'Duration',
    operators: NUMERIC_OPERATORS,
    valueType: 'number',
    unit: 's',
  },
  {
    key: 'notes',
    label: 'Notes',
    operators: TEXT_OPERATORS,
    valueType: 'text',
  },
  {
    key: 'source_url',
    label: 'Source URL',
    operators: TEXT_OPERATORS,
    valueType: 'text',
  },
  {
    key: 'color',
    label: 'Color',
    operators: [
      { value: 'contains', label: 'contains' },
    ],
    valueType: 'color',
  },
];

/** Look up a field definition by key. Falls back to tags if not found. */
export function getFieldDef(key: string): FieldDef {
  return FIELD_DEFS.find((f) => f.key === key) ?? FIELD_DEFS[0];
}

/** Get CmSelect-compatible options for the field dropdown. */
export function getFieldOptions(): { value: string; label: string }[] {
  return FIELD_DEFS.map((f) => ({ value: f.key, label: f.label }));
}

/** Default operator for a field. */
export function defaultOperator(fieldKey: string): string {
  const def = getFieldDef(fieldKey);
  return def.operators[0].value;
}

/** Default value for a field's value type. */
export function defaultValue(fieldKey: string): unknown {
  const def = getFieldDef(fieldKey);
  switch (def.valueType) {
    case 'tags':
    case 'color':
      return undefined; // list-type fields use `values` instead
    case 'number':
    case 'filesize':
      return 0;
    case 'date':
      return new Date().toISOString().slice(0, 10);
    case 'select':
      return def.selectOptions?.[0]?.value ?? '';
    case 'text':
      return '';
  }
}

/** Whether a field uses the `values` array (CSV list) instead of `value`. */
export function isListField(fieldKey: string): boolean {
  const def = getFieldDef(fieldKey);
  return def.valueType === 'tags' || def.valueType === 'color';
}

/** Filesize unit options for the filesize value type. */
export const FILESIZE_UNITS = [
  { value: 'B', label: 'Bytes' },
  { value: 'KB', label: 'Kilobytes' },
  { value: 'MB', label: 'Megabytes' },
  { value: 'GB', label: 'Gigabytes' },
];

export const RATING_OPTIONS = [
  { value: '0', label: 'Unrated' },
  { value: '1', label: '★☆☆☆☆' },
  { value: '2', label: '★★☆☆☆' },
  { value: '3', label: '★★★☆☆' },
  { value: '4', label: '★★★★☆' },
  { value: '5', label: '★★★★★' },
];
