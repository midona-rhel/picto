import {
  IconTag,
  IconFile,
  IconStarFilled,
  IconRuler2,
  IconCalendar,
  IconCursorText,
  IconArrowsHorizontal,
  IconArrowsVertical,
  IconAspectRatio,
  IconClock,
  IconVolume,
  IconNotes,
  IconLink,
  IconEye,
  IconPalette,
  IconRectangle,
} from '@tabler/icons-react';

export interface FieldConfig {
  key: string;
  label: string;
  icon: typeof IconTag;
  operators: { value: string; label: string }[];
  valueType: 'tags' | 'text' | 'number' | 'date' | 'bool' | 'select' | 'color' | 'filesize';
  selectOptions?: { value: string; label: string }[];
  unit?: string;
  hasSecondValue?: boolean;
}

const TEXT_OPS = [
  { value: 'is', label: 'is' },
  { value: 'is_not', label: 'is not' },
  { value: 'contains', label: 'contains' },
  { value: 'does_not_contain', label: 'does not contain' },
  { value: 'starts_with', label: 'starts with' },
  { value: 'ends_with', label: 'ends with' },
  { value: 'is_empty', label: 'is empty' },
  { value: 'is_not_empty', label: 'is not empty' },
];

const NUM_OPS = [
  { value: 'eq', label: '=' },
  { value: 'neq', label: '!=' },
  { value: 'gt', label: '>' },
  { value: 'gte', label: '>=' },
  { value: 'lt', label: '<' },
  { value: 'lte', label: '<=' },
  { value: 'between', label: 'between' },
];

const DATE_OPS = [
  { value: 'is', label: 'is' },
  { value: 'before', label: 'before' },
  { value: 'after', label: 'after' },
  { value: 'in_last', label: 'in the last' },
  { value: 'not_in_last', label: 'not in the last' },
  { value: 'between', label: 'between' },
  { value: 'is_set', label: 'is set' },
  { value: 'is_not_set', label: 'is not set' },
];

const TAG_OPS = [
  { value: 'include', label: 'include' },
  { value: 'do_not_include', label: 'do not include' },
  { value: 'include_all', label: 'include all of' },
  { value: 'include_any', label: 'include any of' },
];

const BOOL_OPS = [
  { value: 'is', label: 'is' },
];

const SHAPE_OPTIONS = [
  { value: 'landscape', label: 'Landscape' },
  { value: 'portrait', label: 'Portrait' },
  { value: 'square', label: 'Square' },
];

const COMMON_MIME_OPTIONS = [
  { value: 'image/jpeg', label: 'JPEG' },
  { value: 'image/png', label: 'PNG' },
  { value: 'image/gif', label: 'GIF' },
  { value: 'image/webp', label: 'WebP' },
  { value: 'image/bmp', label: 'BMP' },
  { value: 'image/tiff', label: 'TIFF' },
  { value: 'image/svg+xml', label: 'SVG' },
  { value: 'video/mp4', label: 'MP4' },
  { value: 'video/webm', label: 'WebM' },
  { value: 'video/quicktime', label: 'MOV' },
  { value: 'video/x-matroska', label: 'MKV' },
];

export const FIELD_CONFIGS: FieldConfig[] = [
  {
    key: 'tags',
    label: 'Tags',
    icon: IconTag,
    operators: TAG_OPS,
    valueType: 'tags',
  },
  {
    key: 'file_type',
    label: 'File Type',
    icon: IconFile,
    operators: TEXT_OPS.slice(0, 2), // is / is not
    valueType: 'select',
    selectOptions: COMMON_MIME_OPTIONS,
  },
  {
    key: 'rating',
    label: 'Rating',
    icon: IconStarFilled,
    operators: NUM_OPS,
    valueType: 'number',
    unit: '/ 10',
  },
  {
    key: 'file_size',
    label: 'File Size',
    icon: IconRuler2,
    operators: NUM_OPS,
    valueType: 'filesize',
  },
  {
    key: 'date_imported',
    label: 'Date Imported',
    icon: IconCalendar,
    operators: DATE_OPS,
    valueType: 'date',
  },
  {
    key: 'name',
    label: 'Name',
    icon: IconCursorText,
    operators: TEXT_OPS,
    valueType: 'text',
  },
  {
    key: 'width',
    label: 'Width',
    icon: IconArrowsHorizontal,
    operators: NUM_OPS,
    valueType: 'number',
    unit: 'px',
  },
  {
    key: 'height',
    label: 'Height',
    icon: IconArrowsVertical,
    operators: NUM_OPS,
    valueType: 'number',
    unit: 'px',
  },
  {
    key: 'aspect_ratio',
    label: 'Aspect Ratio',
    icon: IconAspectRatio,
    operators: NUM_OPS,
    valueType: 'number',
  },
  {
    key: 'duration',
    label: 'Duration',
    icon: IconClock,
    operators: NUM_OPS,
    valueType: 'number',
    unit: 's',
  },
  {
    key: 'has_audio',
    label: 'Has Audio',
    icon: IconVolume,
    operators: BOOL_OPS,
    valueType: 'bool',
  },
  {
    key: 'notes',
    label: 'Notes',
    icon: IconNotes,
    operators: TEXT_OPS,
    valueType: 'text',
  },
  {
    key: 'source_url',
    label: 'Source URL',
    icon: IconLink,
    operators: TEXT_OPS,
    valueType: 'text',
  },
  {
    key: 'view_count',
    label: 'View Count',
    icon: IconEye,
    operators: NUM_OPS,
    valueType: 'number',
  },
  {
    key: 'color',
    label: 'Dominant Color',
    icon: IconPalette,
    operators: [{ value: 'contains', label: 'contains' }],
    valueType: 'color',
  },
  {
    key: 'shape',
    label: 'Shape',
    icon: IconRectangle,
    operators: TEXT_OPS.slice(0, 2), // is / is not
    valueType: 'select',
    selectOptions: SHAPE_OPTIONS,
  },
];

export const FIELD_MAP = new Map(FIELD_CONFIGS.map((f) => [f.key, f]));

export function getFieldConfig(key: string): FieldConfig | undefined {
  return FIELD_MAP.get(key);
}
