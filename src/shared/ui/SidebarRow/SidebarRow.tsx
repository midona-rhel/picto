/**
 * Canonical sidebar row — one component family for all sidebar node types.
 *
 * Renders system scopes, folders, smart folders, and section headers
 * with the same visual structure. Differences are handled by props,
 * not by separate component hierarchies.
 */

import type { ReactNode } from 'react';
import { useAtomValue } from 'jotai';
import { IconChevronRight, IconPlus } from '@tabler/icons-react';
import { showTreeGuidesAtom } from '../../../state/navigation';
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
  active?: boolean;
  dropTarget?: boolean;
  indent?: number;
  hasChildren?: boolean;
  expanded?: boolean;
  /** For each ancestor level, true if a vertical line should continue (more siblings at that depth). */
  treeLines?: boolean[];
  /** True if this is the last child of its parent. */
  isLastChild?: boolean;
  onToggleExpand?: () => void;
  onClick?: () => void;
  onContextMenu?: (e: React.MouseEvent) => void;
  onPointerDown?: (e: React.PointerEvent) => void;
  /** Data attribute for drag-and-drop targeting (e.g. folder ID or status code). */
  dropDataAttr?: { key: string; value: string };
  /** Highlight when this row's context menu is open (but row isn't active). */
  contextHighlight?: boolean;
  /** Drop position indicator for folder drag reorder. */
  dropPosition?: 'before' | 'inside' | 'after';
  children?: ReactNode;
}

const INDENT_PX = 20;

function StandardRow({
  icon,
  label,
  count,
  active,
  dropTarget,
  indent = 0,
  hasChildren,
  expanded,
  treeLines,
  isLastChild,
  onToggleExpand,
  onClick,
  onContextMenu,
  onPointerDown,
  dropDataAttr,
  contextHighlight,
  dropPosition,
  children,
}: RowProps) {
  const cls = [
    styles.row,
    active ? styles.active : '',
    dropTarget ? styles.dropTarget : '',
    contextHighlight && !active ? styles.contextHighlight : '',
    dropPosition === 'before' ? styles.dropBefore : '',
    dropPosition === 'inside' ? styles.dropInside : '',
    dropPosition === 'after' ? styles.dropAfter : '',
  ].filter(Boolean).join(' ');

  const showGuides = useAtomValue(showTreeGuidesAtom);

  const rowStyle: React.CSSProperties | undefined = indent > 0
    ? { paddingLeft: indent * INDENT_PX, '--row-inset': `${(indent * INDENT_PX) + 1}px` } as React.CSSProperties
    : undefined;

  // T-shape if not last child of parent, L-shape if last child
  const useLShape = indent > 0 && (isLastChild ?? true);

  return (
    <div
      className={cls}
      style={rowStyle}
      onClick={onClick}
      onContextMenu={onContextMenu}
      onPointerDown={onPointerDown}
      {...(dropDataAttr ? { [`data-${dropDataAttr.key}`]: dropDataAttr.value } : {})}
    >
      {/* Tree guide lines — continuation at column (d-1) for siblings at indent d.
          Skip d=0: root-level items don't get tree connectors. */}
      {showGuides && indent > 0 && treeLines && treeLines.map((continues, d) => (continues && d > 0) ? (
        <svg
          key={d}
          className={styles.treeLine}
          style={{ left: (d - 1) * INDENT_PX + INDENT_PX / 2 + 1 }}
          viewBox="0 0 10 26"
          preserveAspectRatio="none"
          fill="none"
        >
          <line x1="3.5" y1="0" x2="3.5" y2="26" stroke="var(--color-text-tertiary)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
        </svg>
      ) : null)}
      {/* Branch connector — T (not last) or L (last child with no continuation) */}
      {showGuides && indent > 0 && (
        <svg
          className={styles.treeBranch}
          style={{ left: (indent - 1) * INDENT_PX + INDENT_PX / 2 + 1 }}
          viewBox="0 0 10 26"
          preserveAspectRatio="none"
          fill="none"
        >
          {useLShape ? (
            <path d="M3.5 0 V9.5 A3.5 3.5 0 0 0 7 13 H15" stroke="var(--color-text-tertiary)" strokeWidth="1" fill="none" vectorEffect="non-scaling-stroke" />
          ) : (
            <>
              <line x1="3.5" y1="0" x2="3.5" y2="26" stroke="var(--color-text-tertiary)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
              <line x1="4" y1="13" x2="15" y2="13" stroke="var(--color-text-tertiary)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
            </>
          )}
        </svg>
      )}
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
        <span className={styles.count}>
          {count.toLocaleString()}
        </span>
      )}
    </div>
  );
}

// ── Unified export ───────────────────────────────────────────────

export type SidebarRowProps = SectionProps | (RowProps & { variant?: 'system' | 'folder' | 'smart_folder' });
export { INDENT_PX as SIDEBAR_INDENT_PX };

export function SidebarRow(props: SidebarRowProps) {
  if (props.variant === 'section') {
    return <SectionRow {...props} />;
  }
  return <StandardRow {...props} />;
}
