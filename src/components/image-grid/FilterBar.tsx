import { useCallback, useRef, useState, useEffect, type ComponentType, type ReactNode } from 'react';
import {
  IconTag,
  IconStar,
  IconPalette,
  IconPhoto,
  IconFolder,
  IconX,
  IconCheck,
} from '@tabler/icons-react';
import { ColorPicker, Slider } from '@mantine/core';
import {
  useFilterStore,
  useActiveFilterCount,
  type FilterLogicMode,
  type MimeFilterKey,
} from '../../stores/filterStore';
import { ContextMenu, useContextMenu, type ContextMenuEntry } from '../ui/ContextMenu';
import { TagSelectService } from '../tags/tagSelectService';
import type { TagFilterLogicMode } from '../tags/tagSelectTypes';
import { FolderPickerService } from '../../services/folderPickerService';
import { TextButton } from '../ui/TextButton';
import { buildColorFilterMenu, buildRatingFilterMenu, buildTypesFilterMenu } from '../ui/context-actions/filterActions';
import st from './FilterBar.module.css';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function FilterPill({ pillRef, icon: Icon, iconNode, label, value, isActive, onClick, onClear }: {
  pillRef?: React.Ref<HTMLButtonElement>;
  icon?: ComponentType<any>;
  iconNode?: ReactNode;
  label: string;
  value?: string;
  isActive: boolean;
  onClick: () => void;
  onClear?: () => void;
}) {
  return (
    <div className={st.filterItem}>
      <button
        ref={pillRef}
        className={`${st.pill} ${isActive ? st.pillActive : ''}`}
        onClick={onClick}
      >
        {Icon ? <Icon size={13} className={st.pillIcon} /> : iconNode}
        <span className={st.pillLabel}>{label}</span>
        {value && <span className={st.pillValue}>{value}</span>}
      </button>
      {isActive && onClear && (
        <div className={st.clearBtn} onClick={(e) => { e.stopPropagation(); onClear(); }}>
          <IconX size={10} />
        </div>
      )}
    </div>
  );
}

// Preset palette colors
const PALETTE_COLORS = [
  '#111111', '#FFFFFF', '#9E9E9E', '#A48057',
  '#FC85B3', '#FF2727', '#FFA34B', '#FFD534',
  '#47C595', '#51C4C4', '#2B76E7', '#6D50ED',
];

const SOLID_GRAY_HEX = '#808080';

const RATING_OPTIONS: { label: string; value: number | null }[] = [
  { label: 'Any rating', value: null },
  { label: '1+ star', value: 1 },
  { label: '2+ stars', value: 2 },
  { label: '3+ stars', value: 3 },
  { label: '4+ stars', value: 4 },
  { label: '5 stars only', value: 5 },
];

const MIME_OPTIONS: { label: string; key: MimeFilterKey }[] = [
  { label: 'Images', key: 'images' },
  { label: 'Videos', key: 'videos' },
  { label: 'GIFs', key: 'gifs' },
  { label: 'Audio', key: 'audio' },
];

interface FilterBarProps {
  visible: boolean;
  showSidebar: boolean;
  showInspector: boolean;
  searchTags?: string[];
  excludedSearchTags?: string[];
  tagLogicMode?: TagFilterLogicMode;
  onSearchTagsChange?: (tags: string[]) => void;
  onExcludedSearchTagsChange?: (tags: string[]) => void;
  onTagLogicModeChange?: (mode: TagFilterLogicMode) => void;
}

export function FilterBar({
  visible,
  showSidebar,
  showInspector,
  searchTags,
  excludedSearchTags,
  tagLogicMode = 'OR',
  onSearchTagsChange,
  onExcludedSearchTagsChange,
  onTagLogicModeChange,
}: FilterBarProps) {
  const ratingFilter = useFilterStore((s) => s.ratingFilter);
  const mimeFilter = useFilterStore((s) => s.mimeFilter);
  const colorFilter = useFilterStore((s) => s.colorFilter);
  const normalizedColorFilter =
    colorFilter && /^#[0-9a-fA-F]{6}$/.test(colorFilter) ? colorFilter.toUpperCase() : null;
  const setRatingFilter = useFilterStore((s) => s.setRatingFilter);
  const setColorFilter = useFilterStore((s) => s.setColorFilter);
  const setColorAccuracy = useFilterStore((s) => s.setColorAccuracy);
  const folderFilter = useFilterStore((s) => s.folderFilter);
  const folderFilterMode = useFilterStore((s) => s.folderFilterMode);
  const setFolderFilterMode = useFilterStore((s) => s.setFolderFilterMode);
  const includeFolderFilter = useFilterStore((s) => s.includeFolderFilter);
  const excludeFolderFilter = useFilterStore((s) => s.excludeFolderFilter);
  const clearFolderFilter = useFilterStore((s) => s.clearFolderFilter);
  const clearMimeFilter = useFilterStore((s) => s.clearMimeFilter);
  const clearAllFilters = useFilterStore((s) => s.clearAllFilters);
  const activeFilterCount = useActiveFilterCount();

  const activeTagCount = (searchTags?.length ?? 0) + (excludedSearchTags?.length ?? 0);
  const hasActiveTags = activeTagCount > 0;

  const tagsPillRef = useRef<HTMLButtonElement>(null);
  const searchTagsRef = useRef(searchTags);
  searchTagsRef.current = searchTags;
  const excludedTagsRef = useRef(excludedSearchTags);
  excludedTagsRef.current = excludedSearchTags;
  const tagLogicModeRef = useRef(tagLogicMode);
  tagLogicModeRef.current = tagLogicMode;
  const onSearchTagsChangeRef = useRef(onSearchTagsChange);
  onSearchTagsChangeRef.current = onSearchTagsChange;
  const onExcludedSearchTagsChangeRef = useRef(onExcludedSearchTagsChange);
  onExcludedSearchTagsChangeRef.current = onExcludedSearchTagsChange;
  const onTagLogicModeChangeRef = useRef(onTagLogicModeChange);
  onTagLogicModeChangeRef.current = onTagLogicModeChange;

  const handleTagsPill = useCallback(() => {
    if (!tagsPillRef.current || !onSearchTagsChangeRef.current) return;
    TagSelectService.open({
      anchorEl: tagsPillRef.current,
      selectedTags: searchTagsRef.current ?? [],
      excludedTags: excludedTagsRef.current ?? [],
      logicMode: tagLogicModeRef.current,
      onToggle: (tag, added) => {
        const fn = onSearchTagsChangeRef.current;
        if (!fn) return;
        const current = searchTagsRef.current ?? [];
        if (added) {
          // Remove from excluded if adding
          const nextExcluded = (excludedTagsRef.current ?? []).filter((t) => t !== tag);
          onExcludedSearchTagsChangeRef.current?.(nextExcluded);
          if (!current.includes(tag)) fn([...current, tag]);
        } else {
          fn(current.filter((t) => t !== tag));
        }
      },
      onExclude: (tag) => {
        // Remove from included
        const fn = onSearchTagsChangeRef.current;
        if (fn) {
          const current = searchTagsRef.current ?? [];
          if (current.includes(tag)) fn(current.filter((t) => t !== tag));
        }
        // Toggle excluded
        const ex = excludedTagsRef.current ?? [];
        let nextExcluded: string[];
        if (ex.includes(tag)) {
          nextExcluded = ex.filter((t) => t !== tag);
        } else {
          nextExcluded = [...ex, tag];
        }
        onExcludedSearchTagsChangeRef.current?.(nextExcluded);
      },
      onExcludedTagsChange: (tags) => onExcludedSearchTagsChangeRef.current?.(tags),
      onLogicChange: (mode) => onTagLogicModeChangeRef.current?.(mode),
      onClose: () => {},
    });
  }, []);

  const foldersPillRef = useRef<HTMLButtonElement>(null);
  const folderFilterRef = useRef(folderFilter);
  folderFilterRef.current = folderFilter;

  const handleFoldersPill = useCallback(() => {
    if (!foldersPillRef.current) return;
    FolderPickerService.open({
      anchorEl: foldersPillRef.current,
      selectedFolderIds: [...folderFilterRef.current.includes.keys()],
      excludedFolderIds: [...folderFilterRef.current.excludes.keys()],
      logicMode: folderFilterMode,
      onToggle: (folderId, folderName) => {
        includeFolderFilter(folderId, folderName);
      },
      onExclude: (folderId, folderName) => {
        excludeFolderFilter(folderId, folderName);
      },
      onLogicChange: (mode) => {
        setFolderFilterMode(mode as FilterLogicMode);
      },
    });
  }, [includeFolderFilter, excludeFolderFilter, folderFilterMode, setFolderFilterMode]);

  const ratingMenu = useContextMenu();
  const ratingPillRef = useRef<HTMLButtonElement>(null);

  const handleRatingPill = useCallback(() => {
    if (!ratingPillRef.current) return;
    const rect = ratingPillRef.current.getBoundingClientRect();
    const items: ContextMenuEntry[] = buildRatingFilterMenu(RATING_OPTIONS, ratingFilter, setRatingFilter);
    ratingMenu.openAt({ x: rect.left, y: rect.bottom + 4 }, items);
  }, [ratingFilter, setRatingFilter, ratingMenu]);

  const typesMenu = useContextMenu();
  const typesPillRef = useRef<HTMLButtonElement>(null);

  const handleTypesPill = useCallback(() => {
    if (!typesPillRef.current) return;
    const rect = typesPillRef.current.getBoundingClientRect();
    const items: ContextMenuEntry[] = buildTypesFilterMenu(() => <TypesPanel />);
    typesMenu.openAt({ x: rect.left, y: rect.bottom + 4 }, items);
  }, [typesMenu]);

  const colorMenu = useContextMenu();
  const colorPillRef = useRef<HTMLButtonElement>(null);

  const handleColorPill = useCallback(() => {
    if (!colorPillRef.current) return;
    const rect = colorPillRef.current.getBoundingClientRect();
    const items: ContextMenuEntry[] = buildColorFilterMenu(() => <ColorPanel />);
    colorMenu.openAt({ x: rect.left, y: rect.bottom + 4 }, items);
  }, [colorFilter, colorMenu]);

  function ColorPanel() {
    const currentColor = useFilterStore((s) => s.colorFilter);
    const tolerance = useFilterStore((s) => s.colorAccuracy);
    const normalizedCurrentHex =
      currentColor && /^#[0-9a-fA-F]{6}$/.test(currentColor)
        ? currentColor.toUpperCase()
        : null;
    const [localHex, setLocalHex] = useState(normalizedCurrentHex ?? '');

    useEffect(() => {
      setLocalHex(normalizedCurrentHex ?? '');
    }, [normalizedCurrentHex]);

    const applyHex = (value: string) => {
      const clean = value.replace('#', '').trim();
      if (/^[0-9a-fA-F]{6}$/.test(clean)) {
        setColorFilter('#' + clean.toUpperCase());
      }
    };

    return (
      <div className={st.colorFilter}>
        <div className={st.colorPickerWrap}>
          <ColorPicker
            format="hex"
            value={normalizedCurrentHex ?? '#FF0000'}
            onChange={(hex) => {
              setColorFilter(hex.toUpperCase());
              setLocalHex(hex.toUpperCase());
            }}
            size="sm"
            fullWidth
          />
        </div>

        <div className={st.palettes}>
          <div
            className={`${st.palette} ${st.paletteNone} ${normalizedCurrentHex === null ? st.paletteActive : ''}`}
            title="No color filter"
            onClick={() => { setColorFilter(null); }}
          />
          <div
            className={`${st.palette} ${normalizedCurrentHex === SOLID_GRAY_HEX ? st.paletteActive : ''}`}
            style={{ backgroundColor: SOLID_GRAY_HEX }}
            title={SOLID_GRAY_HEX}
            onClick={() => { setColorFilter(SOLID_GRAY_HEX); setLocalHex(SOLID_GRAY_HEX); }}
          />
          {PALETTE_COLORS.map((hex) => (
            <div
              key={hex}
              className={`${st.palette} ${normalizedCurrentHex === hex ? st.paletteActive : ''}`}
              style={{ backgroundColor: hex }}
              title={hex}
              onClick={() => { setColorFilter(hex); setLocalHex(hex); }}
            />
          ))}
        </div>

        <div className={st.hexRow}>
          <div
            className={st.hexPreview}
            style={{ backgroundColor: normalizedCurrentHex ?? 'transparent' }}
          />
          <input
            className={st.hexInput}
            type="text"
            maxLength={7}
            placeholder="#FF0000"
            value={localHex}
            onChange={(e) => {
              const v = e.target.value;
              setLocalHex(v);
              if (/^#?[0-9a-fA-F]{6}$/.test(v)) applyHex(v);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') { applyHex(localHex); }
              e.stopPropagation();
            }}
          />
        </div>

        {normalizedCurrentHex && (
          <div className={st.accuracyRow}>
            <span className={st.accuracyLabel}>Tolerance</span>
            <div className={st.accuracySliderWrap}>
              <Slider
                value={tolerance}
                onChange={(v) => setColorAccuracy(v)}
                min={1} max={30} step={1}
                label={(v) => `${v}%`}
                color="gray"
              />
            </div>
            <span className={st.accuracyValue}>{tolerance}%</span>
          </div>
        )}
      </div>
    );
  }

  function TypesPanel() {
    const mime = useFilterStore((s) => s.mimeFilter);
    const toggle = useFilterStore((s) => s.toggleMimeFilter);

    return (
      <div className={st.typesPanel}>
        {MIME_OPTIONS.map((opt) => {
          const checked = mime.has(opt.key);
          return (
            <div
              key={opt.key}
              className={`${st.typesItem} ${checked ? st.typesItemChecked : ''}`}
              onClick={() => toggle(opt.key)}
            >
              <span className={`${st.typesCheck} ${checked ? st.typesCheckActive : ''}`}>
                {checked && <IconCheck size={10} strokeWidth={3} />}
              </span>
              <span className={st.typesLabel}>{opt.label}</span>
            </div>
          );
        })}
      </div>
    );
  }

  const ratingLabel = ratingFilter !== null
    ? ratingFilter === 5 ? '5 stars' : `${ratingFilter}+`
    : undefined;

  const typesLabel = mimeFilter.size > 0
    ? mimeFilter.size === 1
      ? MIME_OPTIONS.find((o) => mimeFilter.has(o.key))?.label ?? ''
      : `${mimeFilter.size} types`
    : undefined;

  const barClass = [
    st.filterBar,
    visible && st.filterBarOpen,
    showSidebar && st.filterBarWithSidebar,
    showInspector && st.filterBarWithInspector,
  ].filter(Boolean).join(' ');

  return (
    <>
      <div className={barClass}>
        <FilterPill pillRef={tagsPillRef} icon={IconTag} label="Tags"
          value={hasActiveTags ? `(${activeTagCount})` : undefined}
          isActive={hasActiveTags} onClick={handleTagsPill}
          onClear={() => {
            onSearchTagsChange?.([]);
            onExcludedSearchTagsChange?.([]);
            onTagLogicModeChange?.('OR');
          }} />

        <FilterPill pillRef={foldersPillRef} icon={IconFolder} label="Folders"
          value={(folderFilter.includes.size + folderFilter.excludes.size) > 0 ? `(${folderFilter.includes.size + folderFilter.excludes.size})` : undefined}
          isActive={(folderFilter.includes.size + folderFilter.excludes.size) > 0} onClick={handleFoldersPill}
          onClear={clearFolderFilter} />

        <FilterPill pillRef={ratingPillRef} icon={IconStar} label="Rating"
          value={ratingLabel} isActive={ratingFilter !== null} onClick={handleRatingPill}
          onClear={() => setRatingFilter(null)} />

        <FilterPill pillRef={colorPillRef} label="Color"
          iconNode={normalizedColorFilter
            ? <span className={st.colorDot} style={{ background: normalizedColorFilter }} />
            : <IconPalette size={13} className={st.pillIcon} />}
          isActive={normalizedColorFilter !== null} onClick={handleColorPill}
          onClear={() => setColorFilter(null)} />

        <FilterPill pillRef={typesPillRef} icon={IconPhoto} label="Types"
          value={typesLabel} isActive={mimeFilter.size > 0} onClick={handleTypesPill}
          onClear={clearMimeFilter} />

        <div className={st.filterSpacer} />

        {(activeFilterCount > 0 || hasActiveTags) && (
          <TextButton
            compact
            onClick={() => {
              clearAllFilters();
              onSearchTagsChange?.([]);
              onExcludedSearchTagsChange?.([]);
              onTagLogicModeChange?.('OR');
            }}
          >
            Reset
          </TextButton>
        )}
      </div>

      {ratingMenu.state && (
        <ContextMenu
          items={ratingMenu.state.items}
          position={ratingMenu.state.position}
          onClose={ratingMenu.close}
          searchable={false}
        />
      )}
      {typesMenu.state && (
        <ContextMenu
          items={typesMenu.state.items}
          position={typesMenu.state.position}
          onClose={typesMenu.close}
          searchable={false}
        />
      )}
      {colorMenu.state && (
        <ContextMenu
          items={colorMenu.state.items}
          position={colorMenu.state.position}
          onClose={colorMenu.close}
          searchable={false}
        />
      )}
    </>
  );
}
