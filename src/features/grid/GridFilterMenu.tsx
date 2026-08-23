import { useAtomValue } from 'jotai';
import { useEffect, useState } from 'react';
import type { ItemFilters } from '../../shared/types/generated/application/ItemFilters';
import type { MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import { gridFiltersAtom } from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import styles from './GridFilterMenu.module.css';

const MEDIA_TYPES = ['image', 'video'] as const;

export function countActiveGridFilters(filters: ItemFilters): number {
  return (filters.minimum_rating != null ? 1 : 0)
    + (filters.mime_prefix ? 1 : 0)
    + filters.include_tags.length
    + filters.exclude_tags.length;
}

function FilterPanel({ onApply }: { onApply: () => void }) {
  const filters = useAtomValue(gridFiltersAtom);
  const [rating, setRating] = useState(filters.minimum_rating ?? 0);
  const [mediaType, setMediaType] = useState(filters.mime_prefix?.replace('/', '') ?? '');
  const [tags, setTags] = useState(filters.include_tags.join(', '));

  useEffect(() => {
    setRating(filters.minimum_rating ?? 0);
    setMediaType(filters.mime_prefix?.replace('/', '') ?? '');
    setTags(filters.include_tags.join(', '));
  }, [filters]);

  const apply = () => {
    const parsedTags = tags.split(',').map((tag) => tag.trim()).filter(Boolean);
    gridController.setFilters({
      minimum_rating: rating > 0 ? rating : null,
      mime_prefix: mediaType ? `${mediaType}/` : null,
      include_tags: parsedTags,
      exclude_tags: [],
      text: null,
    });
    onApply();
  };

  const toggleType = (type: string) => setMediaType((current) => current === type ? '' : type);

  return (
    <form className={styles.panel} onSubmit={(event) => { event.preventDefault(); apply(); }}>
      <label className={styles.row}>
        <span>Rating</span>
        <select value={rating} onChange={(event) => setRating(Number(event.target.value))}>
          <option value={0}>Any</option>
          {[1, 2, 3, 4, 5].map((value) => <option key={value} value={value}>{value}+ stars</option>)}
        </select>
      </label>

      <div className={styles.row}>
        <span>Type</span>
        <div className={styles.segmented}>
          {MEDIA_TYPES.map((type) => (
            <button key={type} type="button" className={mediaType === type ? styles.segmentActive : ''} onClick={() => toggleType(type)}>
              {type[0].toUpperCase() + type.slice(1)}
            </button>
          ))}
        </div>
      </div>

      <label className={styles.tags}>
        <span>Tags <small>comma separated</small></span>
        <input value={tags} onChange={(event) => setTags(event.target.value)} placeholder="creator:name, favorite" />
      </label>

      <div className={styles.actions}>
        <button type="button" onClick={() => { setRating(0); setMediaType(''); setTags(''); }}>Clear</button>
        <button type="submit" className={styles.apply}>Apply</button>
      </div>
    </form>
  );
}

export function buildFilterMenuEntries(onApply: () => void): MenuEntry[] {
  return [{ custom: true, key: 'grid-filters', render: () => <FilterPanel onApply={onApply} /> }];
}
