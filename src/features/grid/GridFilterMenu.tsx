import { useAtomValue } from 'jotai';
import { useEffect, useState } from 'react';
import type { QueryFilters } from '../../shared/types/canonical';
import type { MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import { gridFiltersAtom } from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import styles from './GridFilterMenu.module.css';

const MEDIA_TYPES = ['image', 'video', 'audio'] as const;

export function countActiveGridFilters(filters: QueryFilters): number {
  return (filters.rating ? 1 : 0)
    + (filters.entity_types?.length ?? 0)
    + (filters.tags?.length ?? 0);
}

function FilterPanel({ onApply }: { onApply: () => void }) {
  const filters = useAtomValue(gridFiltersAtom);
  const [rating, setRating] = useState(filters.rating?.value ?? 0);
  const [types, setTypes] = useState<string[]>(filters.entity_types ?? []);
  const [tags, setTags] = useState(filters.tags?.map((tag) => tag.tag).join(', ') ?? '');

  useEffect(() => {
    setRating(filters.rating?.value ?? 0);
    setTypes(filters.entity_types ?? []);
    setTags(filters.tags?.map((tag) => tag.tag).join(', ') ?? '');
  }, [filters]);

  const apply = () => {
    const parsedTags = tags.split(',').map((tag) => tag.trim()).filter(Boolean);
    gridController.setFilters({
      rating: rating > 0 ? { value: rating, op: 'gte' } : undefined,
      entity_types: types.length > 0 ? types : undefined,
      tags: parsedTags.length > 0 ? parsedTags.map((tag) => ({ tag, match_mode: 'include' as const })) : undefined,
    });
    onApply();
  };

  const toggleType = (type: string) => {
    setTypes((current) => current.includes(type) ? current.filter((value) => value !== type) : [...current, type]);
  };

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
            <button key={type} type="button" className={types.includes(type) ? styles.segmentActive : ''} onClick={() => toggleType(type)}>
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
        <button type="button" onClick={() => { setRating(0); setTypes([]); setTags(''); }}>Clear</button>
        <button type="submit" className={styles.apply}>Apply</button>
      </div>
    </form>
  );
}

export function buildFilterMenuEntries(onApply: () => void): MenuEntry[] {
  return [{ custom: true, key: 'grid-filters', render: () => <FilterPanel onApply={onApply} /> }];
}
