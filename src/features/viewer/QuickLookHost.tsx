import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { ToolbarCloseIcon } from '../../shared/ui/icons/toolbar-icons';
import { QuickLookNavigation } from './QuickLookNavigation';
import styles from './QuickLookHost.module.css';
import { t } from '../../i18n';

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
  return createPortal(
    <div
      className={styles.overlay}
      data-quick-look-overlay
      data-media-ready={contentReady ? 'true' : 'false'}
    >
      <KbdTooltip label={t("Close")} shortcutId="view.quicklook" position="bottom">
        <button className={styles.closeButton} type="button" onClick={onClose} aria-label={t("Close Quick Look")}>
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
