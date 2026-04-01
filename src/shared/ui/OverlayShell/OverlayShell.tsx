/**
 * OverlayShell — reusable glass floating panel container.
 *
 * Features:
 * - React portal to document.body
 * - Glass panel with backdrop
 * - Draggable by header/footer (cursor: grab, 5px threshold)
 * - Pin mode: hides backdrop, Escape still closes
 * - Boundary checking (12px from viewport edges)
 * - Escape to close (capture phase)
 * - Right-click backdrop to close
 */

import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { IconPin, IconPinFilled } from '@tabler/icons-react';
import styles from './OverlayShell.module.css';

export interface OverlayShellProps {
  open: boolean;
  onClose: () => void;
  width?: number;
  height?: number;
  pinned?: boolean;
  onPinnedChange?: (pinned: boolean) => void;
  /** Anchor position — panel positions relative to this point. */
  anchorPosition?: { x: number; y: number } | null;
  header?: ReactNode;
  footer?: ReactNode;
  children: ReactNode;
}

const MARGIN = 12;
const DRAG_THRESHOLD = 5;

export function OverlayShell({
  open,
  onClose,
  width = 360,
  height = 480,
  pinned = false,
  onPinnedChange,
  anchorPosition,
  header,
  footer,
  children,
}: OverlayShellProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const dragRef = useRef({ active: false, startX: 0, startY: 0, origX: 0, origY: 0 });

  // Whether this panel is right-anchored (opened from inspector)
  const isRightAnchored = anchorPosition != null;

  // Position on open
  // When right-anchored: pos.x = distance from RIGHT edge of viewport (CSS `right`)
  // When centered: pos.x = distance from LEFT edge (CSS `left`)
  useEffect(() => {
    if (!open) return;
    if (anchorPosition) {
      // Right-anchored: panel's right edge at inspector's left edge
      let r = window.innerWidth - anchorPosition.x + MARGIN;
      let y = anchorPosition.y;
      if (r < MARGIN) r = MARGIN;
      if (window.innerWidth - r - width < MARGIN) r = window.innerWidth - width - MARGIN;
      if (y + height > window.innerHeight - MARGIN) y = window.innerHeight - height - MARGIN;
      if (y < MARGIN) y = MARGIN;
      setPos({ x: Math.round(r), y: Math.round(y) });
    } else {
      setPos({
        x: Math.round((window.innerWidth - width) / 2),
        y: Math.round((window.innerHeight - height) / 2),
      });
    }
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  // Escape to close
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.stopPropagation(); onClose(); }
    };
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, [open, onClose]);

  // Drag logic — right-anchored panels invert horizontal drag direction
  const onDragStart = useCallback((e: React.MouseEvent) => {
    if (!panelRef.current || !pos) return;
    if ((e.target as HTMLElement).closest('input, button')) return;
    e.preventDefault();
    const d = dragRef.current;
    d.startX = e.clientX;
    d.startY = e.clientY;
    d.origX = pos.x;
    d.origY = pos.y;
    d.active = false;

    const onMove = (ev: MouseEvent) => {
      const dx = ev.clientX - d.startX;
      const dy = ev.clientY - d.startY;
      if (!d.active && Math.abs(dx) < DRAG_THRESHOLD && Math.abs(dy) < DRAG_THRESHOLD) return;
      d.active = true;
      const maxX = window.innerWidth - width - MARGIN;
      const maxY = window.innerHeight - height - MARGIN;
      setPos({
        // Right-anchored: drag left = increase right distance
        x: Math.max(MARGIN, Math.min(maxX, isRightAnchored ? d.origX - dx : d.origX + dx)),
        y: Math.max(MARGIN, Math.min(maxY, d.origY + dy)),
      });
    };
    const onUp = () => {
      d.active = false;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }, [pos, width, height, isRightAnchored]);

  if (!open || !pos) return null;

  return createPortal(
    <>
      <div
        className={`${styles.backdrop} ${pinned ? styles.backdropHidden : ''}`}
        onClick={onClose}
        onContextMenu={(e) => { e.preventDefault(); onClose(); }}
      />
      <div
        ref={panelRef}
        className={styles.panel}
        style={isRightAnchored
          ? { right: pos.x, top: pos.y, width, height }
          : { left: pos.x, top: pos.y, width, height }
        }
      >
        <div className={styles.header} onMouseDown={onDragStart}>
          {header}
          {onPinnedChange && (
            <button
              className={`${styles.pinBtn} ${pinned ? styles.pinBtnActive : ''}`}
              onClick={() => onPinnedChange(!pinned)}
              type="button"
              title={pinned ? 'Unpin' : 'Pin (keep open)'}
            >
              {pinned ? <IconPinFilled size={14} /> : <IconPin size={14} />}
            </button>
          )}
        </div>
        <div className={styles.body}>
          {children}
        </div>
        {footer && (
          <div className={styles.footer} onMouseDown={onDragStart}>
            {footer}
          </div>
        )}
      </div>
    </>,
    document.body,
  );
}
