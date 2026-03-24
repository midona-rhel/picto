/**
 * Grid tile — renders a single entity in the grid.
 * Compact presentational component. No state, no controllers.
 */

import type { CanonicalEntityGridItem } from '../../shared/types/canonical';
import styles from './GridTile.module.css';

interface GridTileProps {
  item: CanonicalEntityGridItem;
  onClick?: () => void;
}

export function GridTile({ item, onClick }: GridTileProps) {
  const thumbUrl = item.has_thumbnail
    ? `media://host/thumb/${item.entity_hash}.jpg`
    : null;

  return (
    <div
      className={styles.tile}
      style={item.dominant_color_hex ? { backgroundColor: item.dominant_color_hex } : undefined}
      onClick={onClick}
    >
      {thumbUrl ? (
        <img className={styles.thumbnail} src={thumbUrl} loading="lazy" alt="" />
      ) : (
        <div className={styles.placeholder}>
          {item.entity_kind === 'collection' ? 'Collection' : item.mime_type.split('/')[0]}
        </div>
      )}

      {/* Top-right badges */}
      <div className={styles.badges}>
        {item.entity_kind === 'collection' && item.member_count != null && (
          <span className={styles.badge}>{item.member_count}</span>
        )}
        {item.duration_ms != null && (
          <span className={styles.badge}>{formatDuration(item.duration_ms)}</span>
        )}
        {item.has_audio && item.duration_ms != null && (
          <span className={styles.badge}>♪</span>
        )}
      </div>

      {/* Rating stars if set */}
      {item.rating != null && item.rating > 0 && (
        <div className={styles.rating}>{'★'.repeat(item.rating)}</div>
      )}

      {/* Name label at bottom */}
      {item.name && (
        <div className={styles.name}>{item.name}</div>
      )}
    </div>
  );
}

function formatDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const sec = s % 60;
  return `${m}:${sec.toString().padStart(2, '0')}`;
}
