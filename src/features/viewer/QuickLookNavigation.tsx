import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { ToolbarChevronIcon } from '../../shared/ui/icons/toolbar-icons';
import styles from './QuickLookNavigation.module.css';

interface QuickLookNavigationProps {
  currentIndex: number;
  totalCount: number;
  canPrevious: boolean;
  canNext: boolean;
  onNavigate: (delta: number) => void;
}

export function QuickLookNavigation({
  currentIndex,
  totalCount,
  canPrevious,
  canNext,
  onNavigate,
}: QuickLookNavigationProps) {
  return (
    <div className={styles.toolbar}>
      <KbdTooltip label="Previous" shortcutId="view.prevImage">
        <button className={styles.button} type="button" onClick={() => onNavigate(-1)} disabled={!canPrevious}>
          <ToolbarChevronIcon direction="left" />
        </button>
      </KbdTooltip>
      <span className={styles.counter}>{currentIndex + 1} / {totalCount}</span>
      <KbdTooltip label="Next" shortcutId="view.nextImage">
        <button className={styles.button} type="button" onClick={() => onNavigate(1)} disabled={!canNext}>
          <ToolbarChevronIcon direction="right" />
        </button>
      </KbdTooltip>
    </div>
  );
}
