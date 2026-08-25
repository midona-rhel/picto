import { useEffect, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { ToolbarCloseIcon } from '../../shared/ui/icons/toolbar-icons';
import { QuickLookNavigation } from './QuickLookNavigation';
import styles from './QuickLookHost.module.css';

interface QuickLookHostProps {
  children: ReactNode;
  contentReady: boolean;
  currentIndex: number;
  totalCount: number;
  canPrevious: boolean;
  canNext: boolean;
  onNavigate: (delta: number) => void;
  onClose: () => void;
}

export function QuickLookHost({
  children,
  contentReady,
  currentIndex,
  totalCount,
  canPrevious,
  canNext,
  onNavigate,
  onClose,
}: QuickLookHostProps) {
  const [revealed, setRevealed] = useState(contentReady);

  useEffect(() => {
    if (revealed || !contentReady) return;
    const frame = requestAnimationFrame(() => setRevealed(true));
    return () => cancelAnimationFrame(frame);
  }, [contentReady, revealed]);

  return createPortal(
    <div
      className={`${styles.overlay} ${revealed ? styles.open : ''}`}
      data-quick-look-overlay
      data-media-ready={revealed ? 'true' : 'false'}
    >
      <KbdTooltip label="Close" shortcut="Space" position="bottom">
        <button className={styles.closeButton} type="button" onClick={onClose} aria-label="Close Quick Look">
          <ToolbarCloseIcon />
        </button>
      </KbdTooltip>
      {children}
      <QuickLookNavigation
        currentIndex={currentIndex}
        totalCount={totalCount}
        canPrevious={canPrevious}
        canNext={canNext}
        onNavigate={onNavigate}
      />
    </div>,
    document.body,
  );
}
