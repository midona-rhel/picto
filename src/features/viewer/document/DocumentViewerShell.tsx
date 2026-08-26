import type { ReactNode, RefObject } from 'react';
import { useShortcutScope } from '../../../shared/hooks/useShortcutScope';
import { getShortcut, matchesShortcutDef } from '../../../shared/lib/shortcuts';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import {
  TitlebarControlButton,
  TitlebarControlGroup,
} from '../../../shared/ui/TitlebarControls';
import { ToolbarChevronIcon } from '../../../shared/ui/icons/toolbar-icons';
import styles from './DocumentViewerShell.module.css';

interface Props {
  children?: ReactNode;
  error?: string | null;
  viewportRef?: RefObject<HTMLDivElement>;
  pageNumber: number;
  pageCount: number;
  onPreviousPage?: () => void;
  onNextPage?: () => void;
  navigationLabel: string;
  viewportClassName?: string;
}

export function DocumentViewerShell({
  children,
  error,
  viewportRef,
  pageNumber,
  pageCount,
  onPreviousPage,
  onNextPage,
  navigationLabel,
  viewportClassName,
}: Props) {
  const canGoPrevious = pageNumber > 1 && Boolean(onPreviousPage);
  const canGoNext = pageCount > 0 && pageNumber < pageCount && Boolean(onNextPage);
  useShortcutScope((event) => {
    if (canGoPrevious && matchesShortcutDef(event, getShortcut('document.previousPage')!)) {
      event.preventDefault();
      onPreviousPage?.();
      return;
    }
    if (canGoNext && matchesShortcutDef(event, getShortcut('document.nextPage')!)) {
      event.preventDefault();
      onNextPage?.();
    }
  }, { priority: 70 });

  return (
    <div className={styles.root} data-document-viewer>
      <div ref={viewportRef} className={`${styles.viewport} ${viewportClassName ?? ''}`}>
        {error ? <div className={styles.message} role="alert">{error}</div> : children}
      </div>
      <footer className={styles.footer} aria-label={`${navigationLabel} page navigation`}>
        <TitlebarControlGroup>
          <KbdTooltip label="Previous page" shortcutId="document.previousPage">
            <TitlebarControlButton aria-label={`Previous ${navigationLabel} page`} disabled={!canGoPrevious} onClick={onPreviousPage}>
              <ToolbarChevronIcon direction="left" />
            </TitlebarControlButton>
          </KbdTooltip>
          <span className={styles.pageStatus}>Page {pageNumber} of {pageCount || '—'}</span>
          <KbdTooltip label="Next page" shortcutId="document.nextPage">
            <TitlebarControlButton aria-label={`Next ${navigationLabel} page`} disabled={!canGoNext} onClick={onNextPage}>
              <ToolbarChevronIcon direction="right" />
            </TitlebarControlButton>
          </KbdTooltip>
        </TitlebarControlGroup>
      </footer>
    </div>
  );
}
