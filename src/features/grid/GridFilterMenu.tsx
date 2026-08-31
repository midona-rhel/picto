import { useAtomValue, useSetAtom } from 'jotai';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import {
  IconBookmark, IconCalendar, IconClock, IconDeviceFloppy, IconDimensions,
  IconFile, IconFilterPlus, IconFolder, IconLink, IconLock, IconLockOpen, IconNotes,
  IconPhoto, IconRestore, IconStar, IconX,
} from '@tabler/icons-react';
import type { ItemFilters } from '../../shared/lib/itemFilters';
import { ContextMenu, type MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import { gridController } from '../../controllers/gridController';
import { gridFilterLockedAtom, gridFiltersAtom, gridFilterToolbarOpenAtom, gridItemsAtom } from '../../state/grid';
import { folderNodesAtom } from '../../state/sidebar';
import { folderPickerPortalAtom, tagSelectPortalAtom } from '../../state/portals';
import { createEmptyItemFilters } from '../../shared/lib/itemFilters';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { ColorFilterEditor } from '../../shared/ui/ColorFilterEditor';
import { IconChangeColor } from '../../shared/ui/icons/sidebar-menu-icons';
import styles from './GridFilterMenu.module.css';
import { t } from '../../i18n';

type PinnedFilter = 'color' | 'tags' | 'folders' | 'rating' | 'type'
  | 'imported' | 'modified' | 'duration' | 'size' | 'resolution' | 'notes' | 'url';
type FilterMenuKind = 'rating' | 'type' | 'pin' | 'imported' | 'modified'
  | 'color' | 'duration' | 'size' | 'resolution' | 'notes' | 'url' | 'saved';
const DEFAULT_PINNED_FILTERS: PinnedFilter[] = ['color', 'tags', 'folders', 'rating', 'type'];
const ALL_FILTERS: PinnedFilter[] = [
  'color', 'tags', 'folders', 'rating', 'type', 'imported', 'modified',
  'resolution', 'duration', 'size', 'notes', 'url',
];
const PINNED_FILTERS_KEY = 'picto:grid:pinned-filters';
const SAVED_FILTERS_KEY = 'picto:grid:saved-filters';

interface SavedFilter {
  id: string;
  name: string;
  filters: string;
}

function serializeFilters(filters: ItemFilters): string {
  return JSON.stringify(filters, (_key, value) => typeof value === 'bigint'
    ? { $bigint: String(value) }
    : value);
}

function deserializeFilters(value: string): ItemFilters {
  const parsed = JSON.parse(value, (_key, item) => item && typeof item === 'object' && '$bigint' in item
    ? BigInt(item.$bigint)
    : item) as Partial<ItemFilters>;
  return { ...createEmptyItemFilters(), ...parsed };
}

function loadSavedFilters(): SavedFilter[] {
  if (typeof window === 'undefined') return [];
  try {
    const value = JSON.parse(window.localStorage.getItem(SAVED_FILTERS_KEY) ?? '[]');
    return Array.isArray(value)
      ? value.filter((item): item is SavedFilter => typeof item?.id === 'string'
        && typeof item?.name === 'string' && typeof item?.filters === 'string')
      : [];
  } catch {
    return [];
  }
}

function loadPinnedFilters(): Set<PinnedFilter> {
  if (typeof window === 'undefined') return new Set(DEFAULT_PINNED_FILTERS);
  try {
    const parsed = JSON.parse(window.localStorage.getItem(PINNED_FILTERS_KEY) ?? 'null');
    if (!Array.isArray(parsed)) return new Set(DEFAULT_PINNED_FILTERS);
    return new Set(parsed.filter((value): value is PinnedFilter => ALL_FILTERS.includes(value)));
  } catch {
    return new Set(DEFAULT_PINNED_FILTERS);
  }
}

export function countActiveGridFilters(filters: ItemFilters): number {
  return filters.ratings.length
    + filters.include_mime_types.length
    + filters.exclude_mime_types.length
    + (filters.color_hex ? 1 : 0)
    + filters.include_tags.length
    + filters.exclude_tags.length
    + filters.include_folder_ids.length
    + filters.exclude_folder_ids.length
    + (filters.imported_after || filters.imported_before ? 1 : 0)
    + (filters.modified_after || filters.modified_before ? 1 : 0)
    + (filters.min_duration_ms != null || filters.max_duration_ms != null ? 1 : 0)
    + (filters.min_size_bytes != null || filters.max_size_bytes != null ? 1 : 0)
    + (filters.min_width != null || filters.max_width != null
      || filters.min_height != null || filters.max_height != null ? 1 : 0)
    + (filters.notes_present != null || Boolean(filters.notes_contains) ? 1 : 0)
    + (filters.source_url_present != null || Boolean(filters.source_url_contains) ? 1 : 0);
}

function mimeLabel(mimeType: string): string {
  const aliases: Record<string, string> = {
    'image/jpeg': 'JPG',
    'image/svg+xml': 'SVG',
    'video/quicktime': 'MOV',
    'video/x-matroska': 'MKV',
    'audio/mpeg': 'MP3',
    'application/pdf': 'PDF',
  };
  return aliases[mimeType] ?? mimeType.split('/').pop()?.replace(/^x-/, '').toUpperCase() ?? mimeType;
}

function scaledValue(value: bigint, divisor: bigint): string {
  const whole = value / divisor;
  const remainder = value % divisor;
  if (remainder === 0n) return String(whole);
  const decimal = (Number(remainder) / Number(divisor)).toFixed(2).slice(2).replace(/0+$/, '');
  return `${whole}.${decimal}`;
}

function rangeLabel(label: string, minimum: bigint | null, maximum: bigint | null, divisor: bigint, unit: string): string {
  if (minimum == null && maximum == null) return label;
  const min = minimum == null ? '0' : scaledValue(minimum, divisor);
  const max = maximum == null ? '∞' : scaledValue(maximum, divisor);
  return `${min}–${max} ${unit}`;
}

function dateRangeLabel(label: string, after: string | null, before: string | null): string {
  if (!after && !before) return label;
  const inclusiveBefore = before
    ? new Date(new Date(before).getTime() - 86_400_000).toISOString().slice(0, 10)
    : '…';
  return `${after?.slice(0, 10) ?? '…'}–${inclusiveBefore}`;
}

interface FilterControlProps {
  label: string;
  icon: ReactNode;
  active: boolean;
  onOpen: (element: HTMLElement) => void;
  onClear: () => void;
}

function FilterControl({ label, icon, active, onOpen, onClear }: FilterControlProps) {
  return (
    <KbdTooltip label={active ? t("{value0} · Right-click to clear", { value0: label }) : label}>
      <div
        className={`${styles.filterItem} ${active ? styles.filterItemActive : ''}`}
        role="button"
        tabIndex={0}
        onClick={(event) => onOpen(event.currentTarget)}
        onKeyDown={(event) => {
          if (event.key !== 'Enter' && event.key !== ' ') return;
          event.preventDefault();
          onOpen(event.currentTarget);
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          event.stopPropagation();
          onClear();
        }}
      >
        <span className={styles.filterIcon}>{icon}</span>
        <span className={styles.filterLabel}>{label}</span>
        {active ? (
          <button
            type="button"
            className={styles.clearButton}
            aria-label={t("Clear {value0} filter", { value0: label })}
            onClick={(event) => { event.stopPropagation(); onClear(); }}
          >
            <IconX size={12} stroke={2} />
          </button>
        ) : null}
      </div>
    </KbdTooltip>
  );
}

function integerOrNull(value: string, multiplier = 1n): bigint | null {
  const parsed = Number(value);
  return value.trim() && Number.isFinite(parsed) && parsed >= 0
    ? BigInt(Math.round(parsed * Number(multiplier)))
    : null;
}

function useDeferredFilterCommit<T extends unknown[]>(onCommit: (...values: T) => void) {
  const callback = useRef(onCommit);
  const pending = useRef<T | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  callback.current = onCommit;

  const flush = useCallback(() => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    const values = pending.current;
    pending.current = null;
    if (values) callback.current(...values);
  }, []);

  useEffect(() => flush, [flush]);
  return useCallback((...values: T) => {
    pending.current = values;
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(flush, 100);
  }, [flush]);
}

function NumericRangeEditor({
  minimum,
  maximum,
  unit,
  units,
  onCommit,
}: {
  minimum: bigint | null;
  maximum: bigint | null;
  unit: { label: string; multiplier: bigint };
  units?: Array<{ label: string; multiplier: bigint }>;
  onCommit: (minimum: bigint | null, maximum: bigint | null) => void;
}) {
  const [selectedUnit, setSelectedUnit] = useState(unit);
  const [min, setMin] = useState(minimum == null ? '' : scaledValue(minimum, selectedUnit.multiplier));
  const [max, setMax] = useState(maximum == null ? '' : scaledValue(maximum, selectedUnit.multiplier));
  const scheduleCommit = useDeferredFilterCommit(onCommit);
  const commitValues = (nextMin: string, nextMax: string, nextUnit = selectedUnit) => scheduleCommit(
    integerOrNull(nextMin, nextUnit.multiplier),
    integerOrNull(nextMax, nextUnit.multiplier),
  );
  return (
    <div className={styles.rangeEditor}>
      <input aria-label={t("Minimum")} inputMode="decimal" placeholder={t("Min")} value={min} onChange={(event) => { setMin(event.target.value); commitValues(event.target.value, max); }} />
      <span>–</span>
      <input aria-label={t("Maximum")} inputMode="decimal" placeholder={t("Max")} value={max} onChange={(event) => { setMax(event.target.value); commitValues(min, event.target.value); }} />
      {units ? (
        <select
          aria-label={t("Unit")}
          value={selectedUnit.label}
          onChange={(event) => {
            const next = units.find((candidate) => candidate.label === event.target.value) ?? unit;
            setSelectedUnit(next);
            commitValues(min, max, next);
          }}
        >
          {units.map((candidate) => <option key={candidate.label}>{candidate.label}</option>)}
        </select>
      ) : <span className={styles.unitLabel}>{unit.label}</span>}
    </div>
  );
}

function ResolutionEditor({ filters, update }: { filters: ItemFilters; update: (patch: Partial<ItemFilters>) => void }) {
  const row = (label: string, min: bigint | null, max: bigint | null, minKey: 'min_width' | 'min_height', maxKey: 'max_width' | 'max_height') => (
    <label className={styles.resolutionRow}>
      <span>{label}</span>
      <NumericRangeEditor
        minimum={min}
        maximum={max}
        unit={{ label: t("px"), multiplier: 1n }}
        onCommit={(minimum, maximum) => update({ [minKey]: minimum, [maxKey]: maximum })}
      />
    </label>
  );
  return <div className={styles.resolutionEditor}>
    {row('Width', filters.min_width, filters.max_width, 'min_width', 'max_width')}
    {row('Height', filters.min_height, filters.max_height, 'min_height', 'max_height')}
  </div>;
}

function dateValue(value: string | null): string {
  return value?.slice(0, 10) ?? '';
}

function nextDate(value: string): string | null {
  if (!value) return null;
  const date = new Date(`${value}T00:00:00Z`);
  date.setUTCDate(date.getUTCDate() + 1);
  return date.toISOString();
}

function DateRangeEditor({ after, before, onCommit }: {
  after: string | null;
  before: string | null;
  onCommit: (after: string | null, before: string | null) => void;
}) {
  const [start, setStart] = useState(dateValue(after));
  const [end, setEnd] = useState(before ? dateValue(new Date(new Date(before).getTime() - 86_400_000).toISOString()) : '');
  return <div className={styles.dateEditor}>
    <label>{t("From")}<input aria-label={t("From")} type="date" value={start} onChange={(event) => { setStart(event.target.value); onCommit(event.target.value ? `${event.target.value}T00:00:00Z` : null, nextDate(end)); }} /></label>
    <label>{t("To")}<input aria-label={t("To")} type="date" value={end} onChange={(event) => { setEnd(event.target.value); onCommit(start ? `${start}T00:00:00Z` : null, nextDate(event.target.value)); }} /></label>
  </div>;
}

function PresenceKeywordEditor({
  value,
  enabled,
  placeholder,
  onChange,
}: {
  value: string | null;
  enabled: boolean;
  placeholder: string;
  onChange: (value: string | null) => void;
}) {
  const [text, setText] = useState(value ?? '');
  const scheduleCommit = useDeferredFilterCommit(onChange);
  return <textarea
    className={styles.keywordEditor}
    aria-label={placeholder}
    placeholder={placeholder}
    disabled={!enabled}
    value={text}
    onChange={(event) => {
      setText(event.target.value);
      scheduleCommit(event.target.value.trim() || null);
    }}
  />;
}

export function GridFilterToolbar() {
  const open = useAtomValue(gridFilterToolbarOpenAtom);
  const filters = useAtomValue(gridFiltersAtom);
  const filterLocked = useAtomValue(gridFilterLockedAtom);
  const setFilterLocked = useSetAtom(gridFilterLockedAtom);
  const items = useAtomValue(gridItemsAtom);
  const folderNodes = useAtomValue(folderNodesAtom);
  const setTagPortal = useSetAtom(tagSelectPortalAtom);
  const setFolderPortal = useSetAtom(folderPickerPortalAtom);
  const [pinnedFilters, setPinnedFilters] = useState(loadPinnedFilters);
  const [savedFilters, setSavedFilters] = useState(loadSavedFilters);
  const [saveName, setSaveName] = useState('');
  const [activeMenu, setActiveMenu] = useState<{
    kind: FilterMenuKind;
    position: { x: number; y: number };
  } | null>(null);

  const update = useCallback((patch: Partial<ItemFilters>) => {
    gridController.setFilters({ ...filters, ...patch });
  }, [filters]);

  const openMenu = useCallback((element: HTMLElement, kind: FilterMenuKind) => {
    const rect = element.getBoundingClientRect();
    setActiveMenu({ kind, position: { x: rect.left, y: rect.bottom + 4 } });
  }, []);

  const openTagFilter = useCallback((element: HTMLElement) => {
    const rect = element.getBoundingClientRect();
    setTagPortal({
      open: true,
      anchor: { x: rect.left, y: rect.bottom + 4 },
      anchorPlacement: 'below',
      filterMatchMode: filters.tag_match_mode,
      selectedTagFilters: filters.include_tags,
      excludedTagFilters: filters.exclude_tags,
      onApplyTagFilter: (includeTags, excludeTags, mode) => update({
        include_tags: includeTags,
        exclude_tags: excludeTags,
        tag_match_mode: mode,
      }),
    });
  }, [filters.exclude_tags, filters.include_tags, filters.tag_match_mode, setTagPortal, update]);

  const openFolderFilter = useCallback((element: HTMLElement) => {
    const rect = element.getBoundingClientRect();
    setFolderPortal({
      open: true,
      anchor: { x: rect.left, y: rect.bottom + 4 },
      anchorPlacement: 'below',
      filterMatchMode: filters.folder_match_mode,
      selectedFolderIds: filters.include_folder_ids,
      excludedFolderIds: filters.exclude_folder_ids,
      onApplyFolderFilter: (includeFolderIds, excludeFolderIds, mode) => update({
        include_folder_ids: includeFolderIds,
        exclude_folder_ids: excludeFolderIds,
        folder_match_mode: mode,
      }),
    });
  }, [filters.exclude_folder_ids, filters.folder_match_mode, filters.include_folder_ids, setFolderPortal, update]);

  const togglePinnedFilter = useCallback((filter: PinnedFilter) => {
    setPinnedFilters((current) => {
      const next = new Set(current);
      if (next.has(filter)) next.delete(filter); else next.add(filter);
      window.localStorage.setItem(PINNED_FILTERS_KEY, JSON.stringify([...next]));
      return next;
    });
  }, []);

  const persistSavedFilters = useCallback((next: SavedFilter[]) => {
    setSavedFilters(next);
    window.localStorage.setItem(SAVED_FILTERS_KEY, JSON.stringify(next));
  }, []);

  const saveCurrentFilter = useCallback(() => {
    const name = saveName.trim();
    if (!name || countActiveGridFilters(filters) === 0) return;
    persistSavedFilters([...savedFilters, {
      id: globalThis.crypto?.randomUUID?.() ?? `${Date.now()}`,
      name,
      filters: serializeFilters(filters),
    }]);
    setSaveName('');
  }, [filters, persistSavedFilters, saveName, savedFilters]);

  if (!open) return null;

  const tagLabels = [
    ...filters.include_tags.map((tag) => tag.name),
    ...filters.exclude_tags.map((tag) => `Not ${tag.name}`),
  ];
  const folderNames = new Map(folderNodes.map((node) => [Number(node.id.slice(7)), node.name]));
  const folderLabels = [
    ...filters.include_folder_ids.map((id) => folderNames.get(id) ?? `Folder ${id}`),
    ...filters.exclude_folder_ids.map((id) => `Not ${folderNames.get(id) ?? `Folder ${id}`}`),
  ];
  const mimeTypes = [...new Set([
    ...items.map((item) => item.mime),
    ...filters.include_mime_types,
    ...filters.exclude_mime_types,
  ])].filter(Boolean).sort((left, right) => mimeLabel(left).localeCompare(mimeLabel(right)));
  const typeLabels = [
    ...filters.include_mime_types.map(mimeLabel),
    ...filters.exclude_mime_types.map((mimeType) => `-${mimeLabel(mimeType)}`),
  ];

  const ratingEntries: MenuEntry[] = [5, 4, 3, 2, 1, 0].map((rating) => ({
    label: rating === 0 ? t("Unrated") : t("{value0}{value1}", { value0: '★'.repeat(rating), value1: '☆'.repeat(5 - rating) }),
    checked: filters.ratings.includes(rating),
    keepOpen: true,
    action: () => update({
      ratings: filters.ratings.includes(rating)
        ? filters.ratings.filter((value) => value !== rating)
        : [...filters.ratings, rating],
    }),
  }));
  const typeEntries: MenuEntry[] = mimeTypes.map((mimeType) => ({
    label: mimeLabel(mimeType),
    keywords: mimeType,
    checked: filters.include_mime_types.includes(mimeType),
    excluded: filters.exclude_mime_types.includes(mimeType),
    keepOpen: true,
    action: () => update({
      include_mime_types: filters.include_mime_types.includes(mimeType)
        ? filters.include_mime_types.filter((value) => value !== mimeType)
        : [...filters.include_mime_types, mimeType],
      exclude_mime_types: filters.exclude_mime_types.filter((value) => value !== mimeType),
    }),
    contextAction: () => update({
      include_mime_types: filters.include_mime_types.filter((value) => value !== mimeType),
      exclude_mime_types: filters.exclude_mime_types.includes(mimeType)
        ? filters.exclude_mime_types.filter((value) => value !== mimeType)
        : [...filters.exclude_mime_types, mimeType],
    }),
  }));
  const pinEntries: MenuEntry[] = [
    ['color', 'Color', <IconChangeColor size={15} />],
    ['tags', 'Tags', <IconBookmark size={15} />],
    ['folders', 'Folders', <IconFolder size={15} />],
    ['rating', 'Rating', <IconStar size={15} />],
    ['type', 'Type', <IconPhoto size={15} />],
    ['imported', 'Date Imported', <IconCalendar size={15} />],
    ['modified', 'Date Modified', <IconCalendar size={15} />],
    ['resolution', 'Resolution', <IconDimensions size={15} />],
    ['duration', 'Duration', <IconClock size={15} />],
    ['size', 'File Size', <IconFile size={15} />],
    ['notes', 'Notes', <IconNotes size={15} />],
    ['url', 'URL', <IconLink size={15} />],
  ].map(([value, label, icon]) => ({
    label: label as string,
    icon: icon as ReactNode,
    checked: pinnedFilters.has(value as PinnedFilter),
    keepOpen: true,
    action: () => togglePinnedFilter(value as PinnedFilter),
  }));

  const dateEntries = (kind: 'imported' | 'modified'): MenuEntry[] => {
    const patchKeys = kind === 'imported'
      ? { after: 'imported_after' as const, before: 'imported_before' as const }
      : { after: 'modified_after' as const, before: 'modified_before' as const };
    const setDays = (days: number, endOffsetDays = 1) => {
      const end = new Date();
      end.setUTCHours(0, 0, 0, 0);
      end.setUTCDate(end.getUTCDate() + endOffsetDays);
      const start = new Date(end);
      start.setUTCDate(start.getUTCDate() - days);
      update({ [patchKeys.after]: start.toISOString(), [patchKeys.before]: end.toISOString() });
    };
    return [
      { label: t("Today"), action: () => setDays(1) },
      { label: t("Yesterday"), action: () => setDays(1, 0) },
      { label: t("Last 7 Days"), action: () => setDays(7) },
      { label: t("Last 30 Days"), action: () => setDays(30) },
      { label: t("Last 90 Days"), action: () => setDays(90) },
      { label: t("Last 365 Days"), action: () => setDays(365) },
      { separator: true },
      {
        custom: true,
        key: `${kind}-range`,
        render: () => <DateRangeEditor
          after={filters[patchKeys.after]}
          before={filters[patchKeys.before]}
          onCommit={(after, before) => update({ [patchKeys.after]: after, [patchKeys.before]: before })}
        />,
      },
    ];
  };
  const presenceEntries = (
    kind: 'notes' | 'url',
    presentKey: 'notes_present' | 'source_url_present',
    containsKey: 'notes_contains' | 'source_url_contains',
  ): MenuEntry[] => [
    {
      label: t("Has {value0}", { value0: kind === 'notes' ? 'Notes' : 'URL' }),
      checked: filters[presentKey] === true,
      keepOpen: true,
      action: () => update(filters[presentKey] === true
        ? { [presentKey]: null, [containsKey]: null }
        : { [presentKey]: true }),
    },
    {
      label: t("Has No {value0}", { value0: kind === 'notes' ? 'Notes' : 'URL' }),
      checked: filters[presentKey] === false,
      keepOpen: true,
      action: () => update({ [presentKey]: filters[presentKey] === false ? null : false, [containsKey]: null }),
    },
    {
      custom: true,
      key: `${kind}-keyword`,
      render: () => <PresenceKeywordEditor
        value={filters[containsKey]}
        enabled={filters[presentKey] === true}
        placeholder={kind === 'notes' ? t("Search notes") : t("Search URLs")}
        onChange={(value) => update({ [containsKey]: value })}
      />,
    },
  ];
  const editorEntries: Record<Exclude<FilterMenuKind, 'rating' | 'type' | 'pin' | 'color' | 'saved'>, MenuEntry[]> = {
    imported: dateEntries('imported'),
    modified: dateEntries('modified'),
    duration: [{
      custom: true,
      key: 'duration-range',
      render: () => <NumericRangeEditor
        minimum={filters.min_duration_ms}
        maximum={filters.max_duration_ms}
        unit={{ label: t("s"), multiplier: 1000n }}
        units={[{ label: t("s"), multiplier: 1000n }, { label: t("m"), multiplier: 60_000n }, { label: t("h"), multiplier: 3_600_000n }]}
        onCommit={(minimum, maximum) => update({ min_duration_ms: minimum, max_duration_ms: maximum })}
      />,
    }],
    size: [{
      custom: true,
      key: 'size-range',
      render: () => <NumericRangeEditor
        minimum={filters.min_size_bytes}
        maximum={filters.max_size_bytes}
        unit={{ label: t("MB"), multiplier: 1_000_000n }}
        units={[{ label: t("KB"), multiplier: 1000n }, { label: t("MB"), multiplier: 1_000_000n }]}
        onCommit={(minimum, maximum) => update({ min_size_bytes: minimum, max_size_bytes: maximum })}
      />,
    }],
    resolution: [{
      custom: true,
      key: 'resolution-range',
      render: () => <ResolutionEditor filters={filters} update={update} />,
    }],
    notes: presenceEntries('notes', 'notes_present', 'notes_contains'),
    url: presenceEntries('url', 'source_url_present', 'source_url_contains'),
  };

  const savedFilterEntries: MenuEntry[] = [
    {
      custom: true,
      key: 'save-current-filter',
      render: () => <div className={styles.saveFilterEditor}>
        <input
          aria-label={t("Saved filter name")}
          value={saveName}
          onChange={(event) => setSaveName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return;
            event.preventDefault();
            saveCurrentFilter();
          }}
          placeholder={t("Filter name")}
        />
        <button
          type="button"
          disabled={!saveName.trim() || countActiveGridFilters(filters) === 0}
          onClick={saveCurrentFilter}
        >{t("Save")}</button>
      </div>,
    },
    ...(savedFilters.length > 0 ? [{ separator: true } as MenuEntry] : []),
    ...savedFilters.map((saved): MenuEntry => ({
      label: saved.name,
      keepOpen: false,
      action: () => update(deserializeFilters(saved.filters)),
      contextAction: () => persistSavedFilters(savedFilters.filter((item) => item.id !== saved.id)),
    })),
  ];

  const activeMenuEntries = activeMenu?.kind === 'saved' ? savedFilterEntries
    : activeMenu?.kind === 'color' ? [{
    custom: true as const,
    key: 'color-filter',
    render: () => <ColorFilterEditor
      value={filters.color_hex}
      deltaE={filters.color_delta_e}
      showSensitivity
      onCommit={(colorHex, deltaE) => update({ color_hex: colorHex, color_delta_e: deltaE })}
    />,
  }]
    : activeMenu?.kind === 'rating' ? ratingEntries
    : activeMenu?.kind === 'type' ? typeEntries
    : activeMenu?.kind === 'pin' || !activeMenu ? pinEntries
    : editorEntries[activeMenu.kind];
  const activeMenuWidth = activeMenu?.kind === 'saved' ? 250
    : activeMenu?.kind === 'color' ? 230
    : activeMenu?.kind === 'notes' || activeMenu?.kind === 'url' ? 240
    : activeMenu?.kind === 'imported' || activeMenu?.kind === 'modified' ? 220
    : activeMenu?.kind === 'duration' || activeMenu?.kind === 'size' || activeMenu?.kind === 'resolution' ? 210
    : undefined;

  return (
    <div className={styles.toolbar} data-grid-filter-toolbar="">
      <div className={styles.filterViewport}>
        <div className={styles.filterItems}>
        {(pinnedFilters.has('color') || filters.color_hex) ? <FilterControl
          label={filters.color_hex?.toUpperCase() ?? 'Color'}
          icon={filters.color_hex
            ? <span className={styles.colorDot} style={{ background: filters.color_hex }} />
            : <IconChangeColor size={16} stroke={1.6} />}
          active={Boolean(filters.color_hex)}
          onOpen={(element) => openMenu(element, 'color')}
          onClear={() => update({ color_hex: null })}
        /> : null}
        {(pinnedFilters.has('tags') || tagLabels.length > 0) ? <FilterControl
          label={tagLabels.length ? tagLabels.join(', ') : t("Tags")}
          icon={<IconBookmark size={16} stroke={1.6} />}
          active={tagLabels.length > 0}
          onOpen={openTagFilter}
          onClear={() => update({ include_tags: [], exclude_tags: [] })}
        /> : null}
        {(pinnedFilters.has('folders') || folderLabels.length > 0) ? <FilterControl
          label={folderLabels.length ? folderLabels.join(', ') : t("Folders")}
          icon={<IconFolder size={16} stroke={1.6} />}
          active={folderLabels.length > 0}
          onOpen={openFolderFilter}
          onClear={() => update({ include_folder_ids: [], exclude_folder_ids: [] })}
        /> : null}
        {(pinnedFilters.has('rating') || filters.ratings.length > 0) ? <FilterControl
          label={filters.ratings.length === 0
            ? t("Rating")
            : filters.ratings.map((rating) => rating === 0 ? 'Unrated' : String(rating)).join(', ')}
          icon={<IconStar size={16} stroke={1.6} />}
          active={filters.ratings.length > 0}
          onOpen={(element) => openMenu(element, 'rating')}
          onClear={() => update({ ratings: [] })}
        /> : null}
        {(pinnedFilters.has('type') || typeLabels.length > 0) ? <FilterControl
          label={typeLabels.length > 0 ? typeLabels.join(', ') : t("Type")}
          icon={<IconPhoto size={16} stroke={1.6} />}
          active={typeLabels.length > 0}
          onOpen={(element) => openMenu(element, 'type')}
          onClear={() => update({ include_mime_types: [], exclude_mime_types: [] })}
        /> : null}
        {(pinnedFilters.has('imported') || filters.imported_after || filters.imported_before) ? <FilterControl
          label={dateRangeLabel('Date Imported', filters.imported_after, filters.imported_before)}
          icon={<IconCalendar size={16} stroke={1.6} />}
          active={Boolean(filters.imported_after || filters.imported_before)}
          onOpen={(element) => openMenu(element, 'imported')}
          onClear={() => update({ imported_after: null, imported_before: null })}
        /> : null}
        {(pinnedFilters.has('modified') || filters.modified_after || filters.modified_before) ? <FilterControl
          label={dateRangeLabel('Date Modified', filters.modified_after, filters.modified_before)}
          icon={<IconCalendar size={16} stroke={1.6} />}
          active={Boolean(filters.modified_after || filters.modified_before)}
          onOpen={(element) => openMenu(element, 'modified')}
          onClear={() => update({ modified_after: null, modified_before: null })}
        /> : null}
        {(pinnedFilters.has('resolution') || filters.min_width != null || filters.max_width != null || filters.min_height != null || filters.max_height != null) ? <FilterControl
          label={(filters.min_width == null && filters.max_width == null && filters.min_height == null && filters.max_height == null) ? t("Resolution") : t("{value0}–{value1} × {value2}–{value3}", { value0: String(filters.min_width ?? 0), value1: String(filters.max_width ?? '∞'), value2: String(filters.min_height ?? 0), value3: String(filters.max_height ?? '∞') })}
          icon={<IconDimensions size={16} stroke={1.6} />}
          active={filters.min_width != null || filters.max_width != null || filters.min_height != null || filters.max_height != null}
          onOpen={(element) => openMenu(element, 'resolution')}
          onClear={() => update({ min_width: null, max_width: null, min_height: null, max_height: null })}
        /> : null}
        {(pinnedFilters.has('duration') || filters.min_duration_ms != null || filters.max_duration_ms != null) ? <FilterControl
          label={rangeLabel('Duration', filters.min_duration_ms, filters.max_duration_ms, 1000n, 's')}
          icon={<IconClock size={16} stroke={1.6} />}
          active={filters.min_duration_ms != null || filters.max_duration_ms != null}
          onOpen={(element) => openMenu(element, 'duration')}
          onClear={() => update({ min_duration_ms: null, max_duration_ms: null })}
        /> : null}
        {(pinnedFilters.has('size') || filters.min_size_bytes != null || filters.max_size_bytes != null) ? <FilterControl
          label={rangeLabel('File Size', filters.min_size_bytes, filters.max_size_bytes, 1_000_000n, 'MB')}
          icon={<IconFile size={16} stroke={1.6} />}
          active={filters.min_size_bytes != null || filters.max_size_bytes != null}
          onOpen={(element) => openMenu(element, 'size')}
          onClear={() => update({ min_size_bytes: null, max_size_bytes: null })}
        /> : null}
        {(pinnedFilters.has('notes') || filters.notes_present != null || filters.notes_contains) ? <FilterControl
          label={filters.notes_present === false ? t("Has No Notes") : filters.notes_contains ? t("Notes: {value0}", { value0: filters.notes_contains }) : filters.notes_present ? t("Has Notes") : t("Notes")}
          icon={<IconNotes size={16} stroke={1.6} />}
          active={filters.notes_present != null || Boolean(filters.notes_contains)}
          onOpen={(element) => openMenu(element, 'notes')}
          onClear={() => update({ notes_present: null, notes_contains: null })}
        /> : null}
        {(pinnedFilters.has('url') || filters.source_url_present != null || filters.source_url_contains) ? <FilterControl
          label={filters.source_url_present === false ? t("Has No URL") : filters.source_url_contains ? t("URL: {value0}", { value0: filters.source_url_contains }) : filters.source_url_present ? t("Has URL") : t("URL")}
          icon={<IconLink size={16} stroke={1.6} />}
          active={filters.source_url_present != null || Boolean(filters.source_url_contains)}
          onOpen={(element) => openMenu(element, 'url')}
          onClear={() => update({ source_url_present: null, source_url_contains: null })}
        /> : null}
        <KbdTooltip label={t("Add or remove filter fields")}>
          <button
            type="button"
            className={styles.addButton}
            aria-label={t("Add filter")}
            onClick={(event) => openMenu(event.currentTarget, 'pin')}
          >
            <IconFilterPlus size={16} stroke={1.6} />
          </button>
        </KbdTooltip>
        </div>
      </div>
      <div className={styles.filterRight}>
        <span className={styles.filterSeparator} />
        <KbdTooltip label={t("Save or apply a filter")}>
          <button
            type="button"
            className={styles.filterAction}
            aria-label={t("Saved filters")}
            onClick={(event) => openMenu(event.currentTarget, 'saved')}
          ><IconDeviceFloppy size={16} stroke={1.6} /></button>
        </KbdTooltip>
        <KbdTooltip label={filterLocked ? t("Keep filters on navigation: on") : t("Keep filters on navigation")}>
          <button
            type="button"
            className={`${styles.filterAction} ${filterLocked ? styles.filterActionActive : ''}`}
            aria-label={filterLocked ? t("Unlock filters") : t("Lock filters")}
            aria-pressed={filterLocked}
            onClick={() => setFilterLocked((value) => !value)}
          >{filterLocked ? <IconLock size={16} stroke={1.6} /> : <IconLockOpen size={16} stroke={1.6} />}</button>
        </KbdTooltip>
        <KbdTooltip label={t("Clear all filters")}>
          <button
            type="button"
            className={styles.filterAction}
            aria-label={t("Clear filters")}
            disabled={countActiveGridFilters(filters) === 0}
            onClick={() => gridController.setFilters(createEmptyItemFilters())}
          ><IconRestore size={16} stroke={1.6} /></button>
        </KbdTooltip>
      </div>
      {activeMenu ? (
        <ContextMenu
          entries={activeMenuEntries}
          position={activeMenu.position}
          showSearch={activeMenu.kind === 'type' || activeMenu.kind === 'pin'}
          width={activeMenuWidth}
          onClose={() => setActiveMenu(null)}
        />
      ) : null}
    </div>
  );
}
