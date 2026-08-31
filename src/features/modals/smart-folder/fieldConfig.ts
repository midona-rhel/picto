
import { t } from '../../../i18n';/**
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
  { value: 'include_all', label: t("include all") },
  { value: 'include_any', label: t("include any") },
  { value: 'do_not_include', label: t("do not include") },
];

const NUMERIC_OPERATORS = [
  { value: 'eq', label: t("equals") },
  { value: 'neq', label: t("not equals") },
  { value: 'gt', label: t("greater than") },
  { value: 'gte', label: t("at least") },
  { value: 'lt', label: t("less than") },
  { value: 'lte', label: t("at most") },
  { value: 'between', label: t("between") },
];

const TEXT_OPERATORS = [
  { value: 'contains', label: t("contains") },
  { value: 'is_empty', label: t("is empty") },
  { value: 'is_not_empty', label: t("is not empty") },
];

const DATE_OPERATORS = [
  { value: 'eq', label: t("is") },
  { value: 'gt', label: t("after") },
  { value: 'gte', label: t("on or after") },
  { value: 'lt', label: t("before") },
  { value: 'lte', label: t("on or before") },
  { value: 'between', label: t("between") },
];

export const FIELD_DEFS: FieldDef[] = [
  {
    key: 'tags',
    label: t("Tags"),
    operators: TAG_OPERATORS,
    valueType: 'tags',
  },
  {
    key: 'file_type',
    label: t("File Type"),
    operators: [
      { value: 'is', label: t("is") },
      { value: 'is_not', label: t("is not") },
    ],
    valueType: 'select',
    selectOptions: [
      { value: 'image', label: t("Image") },
      { value: 'video', label: t("Video") },
      { value: 'audio', label: t("Audio") },
      { value: 'image/png', label: t("PNG") },
      { value: 'image/jpeg', label: t("JPEG") },
      { value: 'video/mp4', label: t("MP4") },
    ],
  },
  {
    key: 'rating',
    label: t("Rating"),
    operators: NUMERIC_OPERATORS,
    valueType: 'number',
  },
  {
    key: 'file_size',
    label: t("File Size"),
    operators: NUMERIC_OPERATORS,
    valueType: 'filesize',
  },
  {
    key: 'date_added',
    label: t("Date Imported"),
    operators: DATE_OPERATORS,
    valueType: 'date',
  },
  {
    key: 'date_created',
    label: t("Date Created"),
    operators: DATE_OPERATORS,
    valueType: 'date',
  },
  {
    key: 'date_modified',
    label: t("Date Modified"),
    operators: DATE_OPERATORS,
    valueType: 'date',
  },
  {
    key: 'name',
    label: t("Name"),
    operators: [{ value: 'contains', label: t("contains") }],
    valueType: 'text',
  },
  {
    key: 'width',
    label: t("Width"),
    operators: NUMERIC_OPERATORS,
    valueType: 'number',
    unit: 'px',
  },
  {
    key: 'height',
    label: t("Height"),
    operators: NUMERIC_OPERATORS,
    valueType: 'number',
    unit: 'px',
  },
  {
    key: 'duration',
    label: t("Duration"),
    operators: NUMERIC_OPERATORS,
    valueType: 'number',
    unit: 's',
  },
  {
    key: 'notes',
    label: t("Notes"),
    operators: TEXT_OPERATORS,
    valueType: 'text',
  },
  {
    key: 'source_url',
    label: t("Source URL"),
    operators: TEXT_OPERATORS,
    valueType: 'text',
  },
  {
    key: 'color',
    label: t("Color"),
    operators: [
      { value: 'contains', label: t("contains") },
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
  { value: 'B', label: t("Bytes") },
  { value: 'KB', label: t("Kilobytes") },
  { value: 'MB', label: t("Megabytes") },
  { value: 'GB', label: t("Gigabytes") },
];

export const RATING_OPTIONS = [
  { value: '0', label: t("Unrated") },
  { value: '1', label: '★☆☆☆☆' },
  { value: '2', label: '★★☆☆☆' },
  { value: '3', label: '★★★☆☆' },
  { value: '4', label: '★★★★☆' },
  { value: '5', label: '★★★★★' },
];
