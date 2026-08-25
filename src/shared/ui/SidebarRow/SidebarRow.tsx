/**
 * Canonical sidebar row — one component family for all sidebar node types.
 *
 * Renders system scopes, folders, smart folders, and section headers
 * with the same visual structure. Differences are handled by props,
 * not by separate component hierarchies.
 */

import type { MouseEvent, ReactNode } from 'react';
import { useAtomValue } from 'jotai';
import { IconChevronRight, IconPlus } from '@tabler/icons-react';
import { KbdTooltip } from '../KbdTooltip';
import { showTreeGuidesAtom } from '../../../state/navigation';
import styles from './SidebarRow.module.css';

// ── Section header variant ───────────────────────────────────────

interface SectionProps {
  variant: 'section';
  label: string;
  count?: number | null;
  expanded: boolean;
  onToggle: () => void;
  onAdd?: (event: MouseEvent<HTMLButtonElement>) => void;
  addTooltip?: string;
  addShortcut?: string;
  collapsible?: boolean;
}

function SectionRow({
  label,
  count,
  expanded,
  onToggle,
  onAdd,
  addTooltip,
  addShortcut,
  collapsible = true,
}: SectionProps) {
  return (
    <div
      className={styles.section}
      onClick={onToggle}
      role="button"
      tabIndex={0}
      aria-expanded={expanded}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onToggle();
        }
      }}
    >
      <div className={styles.sectionTitleRow}>
        <span className={styles.sectionMeta}>
          <span className={styles.sectionTitle}>{label}</span>
          {count != null && count > 0 && <span className={styles.sectionCount}> ({count.toLocaleString()})</span>}
        </span>
        <span
          className={`${styles.sectionArrow} ${expanded ? styles.sectionArrowExpanded : ''} ${!collapsible ? styles.sectionArrowStatic : ''}`}
        >
          <IconChevronRight size={11} />
        </span>
      </div>
      {onAdd && (
        <KbdTooltip label={addTooltip ?? 'Add'} shortcut={addShortcut}>
          <button
            className={styles.sectionAddBtn}
            aria-label={addTooltip ?? 'Add'}
            onClick={(e) => { e.stopPropagation(); onAdd(e); }}
          >
            <IconPlus size={14} />
          </button>
        </KbdTooltip>
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
  selected?: boolean;
  dropTarget?: boolean;
  indent?: number;
  hasChildren?: boolean;
  expanded?: boolean;
  /** For each ancestor level, true if a vertical line should continue (more siblings at that depth). */
  treeLines?: boolean[];
  /** True if this is the last child of its parent. */
  isLastChild?: boolean;
  onToggleExpand?: () => void;
  onClick?: (e: React.MouseEvent) => void;
  onDoubleClick?: (e: React.MouseEvent) => void;
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
  variant = 'system',
  icon,
  label,
  count,
  active,
  selected,
  dropTarget,
  indent = 0,
  hasChildren,
  expanded,
  treeLines,
  isLastChild,
  onToggleExpand,
  onClick,
  onDoubleClick,
  onContextMenu,
  onPointerDown,
  dropDataAttr,
  contextHighlight,
  dropPosition,
  children,
}: RowProps) {
  const interactive = !!onClick;
  const cls = [
    styles.row,
    variant === 'system' ? styles.systemRow : '',
    interactive ? styles.rowInteractive : '',
    active ? styles.active : '',
    selected && !active ? styles.selected : '',
    dropTarget ? styles.dropTarget : '',
    contextHighlight && !active ? styles.contextHighlight : '',
    dropPosition === 'before' ? styles.dropBefore : '',
    dropPosition === 'inside' ? styles.dropInside : '',
    dropPosition === 'after' ? styles.dropAfter : '',
  ].filter(Boolean).join(' ');

  const showGuides = useAtomValue(showTreeGuidesAtom);
  const hasVisibleCount = count != null && count > 0;

  const rowIndent = indent * INDENT_PX;
  const rowStyle = {
    '--sidebar-row-indent': `${rowIndent}px`,
  } as React.CSSProperties;

  // T-shape if not last child of parent, L-shape if last child
  const useLShape = indent > 0 && (isLastChild ?? true);

  return (
    <div
      className={cls}
      style={rowStyle}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      role={interactive ? 'button' : undefined}
      tabIndex={interactive ? 0 : undefined}
      aria-current={active ? 'page' : undefined}
      aria-expanded={hasChildren ? expanded : undefined}
      onKeyDown={interactive ? (event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onClick?.(event as unknown as React.MouseEvent);
        }
      } : undefined}
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
          style={{ left: `calc(var(--sidebar-content-inset) + ${(d - 1) * INDENT_PX + INDENT_PX / 2}px)` }}
          viewBox="0 0 10 26"
          preserveAspectRatio="none"
          fill="none"
        >
          <line x1="3.5" y1="0" x2="3.5" y2="26" stroke="var(--sidebar-tree-guide-color)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
        </svg>
      ) : null)}
      {/* Branch connector — T (not last) or L (last child). With arrow cutout when hasChildren. */}
      {showGuides && indent > 0 && (
        <svg
          className={styles.treeBranch}
          style={{ left: `calc(var(--sidebar-content-inset) + ${(indent - 1) * INDENT_PX + INDENT_PX / 2}px)` }}
          viewBox="0 0 10 26"
          preserveAspectRatio="none"
          fill="none"
        >
          {hasChildren ? (
            /* Arrow cutout variants — vertical line has gap Y=9..17, short horizontal arm */
            useLShape ? (
              /* L with cutout: top segment to Y=9, horizontal arm X=8..15 at Y=13 */
              <>
                <line x1="3.5" y1="0" x2="3.5" y2="9" stroke="var(--sidebar-tree-guide-color)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
                <line x1="8" y1="13" x2="15" y2="13" stroke="var(--sidebar-tree-guide-color)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
              </>
            ) : (
              /* T with cutout: top to Y=9, bottom from Y=17, horizontal arm X=8..15 */
              <>
                <line x1="3.5" y1="0" x2="3.5" y2="9" stroke="var(--sidebar-tree-guide-color)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
                <line x1="3.5" y1="17" x2="3.5" y2="26" stroke="var(--sidebar-tree-guide-color)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
                <line x1="8" y1="13" x2="15" y2="13" stroke="var(--sidebar-tree-guide-color)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
              </>
            )
          ) : (
            /* No arrow — standard L/T shapes */
            useLShape ? (
              <path d="M3.5 0 V9.5 A3.5 3.5 0 0 0 7 13 H15" stroke="var(--sidebar-tree-guide-color)" strokeWidth="1" fill="none" vectorEffect="non-scaling-stroke" />
            ) : (
              <>
                <line x1="3.5" y1="0" x2="3.5" y2="26" stroke="var(--sidebar-tree-guide-color)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
                <line x1="4" y1="13" x2="15" y2="13" stroke="var(--sidebar-tree-guide-color)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
              </>
            )
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
      <div className={`${styles.rowContent} ${hasVisibleCount ? styles.rowContentWithCount : ''}`}>
        {icon && <span className={styles.icon}>{icon}</span>}
        {children ?? (label != null && <span className={styles.label}>{label}</span>)}
        {hasVisibleCount && (
          <span className={styles.count}>
            {count.toLocaleString()}
          </span>
        )}
      </div>
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
