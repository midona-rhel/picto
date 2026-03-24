/**
 * Canonical sidebar row — one component family for all sidebar node types.
 *
 * Renders system scopes, folders, smart folders, and section headers
 * with the same visual structure. Differences are handled by props,
 * not by separate component hierarchies.
 */

import type { ReactNode } from 'react';
import { IconChevronRight, IconPlus } from '@tabler/icons-react';
import styles from './SidebarRow.module.css';

// ── Section header variant ───────────────────────────────────────

interface SectionProps {
  variant: 'section';
  label: string;
  expanded: boolean;
  onToggle: () => void;
  onAdd?: () => void;
}

function SectionRow({ label, expanded, onToggle, onAdd }: SectionProps) {
  return (
    <div className={styles.section} onClick={onToggle}>
      <div className={styles.sectionTitleRow}>
        <span className={styles.sectionTitle}>{label}</span>
        <span className={`${styles.sectionArrow} ${expanded ? styles.sectionArrowExpanded : ''}`}>
          <IconChevronRight size={11} />
        </span>
      </div>
      {onAdd && (
        <button
          className={styles.sectionAddBtn}
          onClick={(e) => { e.stopPropagation(); onAdd(); }}
        >
          <IconPlus size={14} />
        </button>
      )}
    </div>
  );
}

// ── Standard row (system, folder, smart folder) ──────────────────

interface RowProps {
  variant?: 'system' | 'folder' | 'smart_folder';
  icon?: ReactNode;
  label?: string;
  count?: number | null;
  countStale?: boolean;
  active?: boolean;
  dropTarget?: boolean;
  indent?: number;
  hasChildren?: boolean;
  expanded?: boolean;
  onToggleExpand?: () => void;
  onClick?: () => void;
  onContextMenu?: (e: React.MouseEvent) => void;
  children?: ReactNode;
}

function StandardRow({
  icon,
  label,
  count,
  countStale,
  active,
  dropTarget,
  indent = 0,
  hasChildren,
  expanded,
  onToggleExpand,
  onClick,
  onContextMenu,
  children,
}: RowProps) {
  const cls = [
    styles.row,
    active ? styles.active : '',
    dropTarget ? styles.dropTarget : '',
  ].filter(Boolean).join(' ');

  const rowStyle: React.CSSProperties | undefined = indent > 0
    ? { paddingLeft: indent * 18, '--row-inset': `${(indent * 18) - 1}px` } as React.CSSProperties
    : undefined;

  return (
    <div
      className={cls}
      style={rowStyle}
      onClick={onClick}
      onContextMenu={onContextMenu}
    >
      {hasChildren && (
        <div
          className={styles.expandArrow}
          onClick={(e) => { e.stopPropagation(); onToggleExpand?.(); }}
        >
          <span className={`${styles.triangle} ${expanded ? styles.expanded : styles.collapsed}`} />
        </div>
      )}
      {icon && <span className={styles.icon}>{icon}</span>}
      {children ?? (label != null && <span className={styles.label}>{label}</span>)}
      {count != null && (
        <span className={`${styles.count} ${countStale ? styles.stale : ''}`}>
          {count.toLocaleString()}
        </span>
      )}
    </div>
  );
}

// ── Unified export ───────────────────────────────────────────────

export type SidebarRowProps = SectionProps | (RowProps & { variant?: 'system' | 'folder' | 'smart_folder' });

export function SidebarRow(props: SidebarRowProps) {
  if (props.variant === 'section') {
    return <SectionRow {...props} />;
  }
  return <StandardRow {...props} />;
}
