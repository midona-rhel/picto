import { useCallback } from 'react';
import { createPortal } from 'react-dom';
import { useGlobalKeydown } from '../../hooks/useGlobalKeydown';
import st from './OverlayShell.module.css';

export interface OverlayShellProps {
  open: boolean;
  onClose: () => void;
  children: React.ReactNode;
  /** When true, backdrop is hidden but Escape still closes. */
  pinned?: boolean;
  /** Close when right-clicking the backdrop (default true). */
  closeOnRightClick?: boolean;
}

export function OverlayShell({
  open,
  onClose,
  children,
  pinned = false,
  closeOnRightClick = true,
}: OverlayShellProps) {
  const onEscape = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    },
    [onClose],
  );

  useGlobalKeydown(onEscape, open, { capture: true });

  if (!open) return null;

  return createPortal(
    <div className="no-drag-region">
      {!pinned && (
        <div
          className={st.backdrop}
          onClick={onClose}
          onContextMenu={
            closeOnRightClick
              ? (e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  onClose();
                }
              : undefined
          }
        />
      )}
      {children}
    </div>,
    document.body,
  );
}
