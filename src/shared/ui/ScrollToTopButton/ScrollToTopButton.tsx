import { IconArrowUp } from '@tabler/icons-react';
import styles from './ScrollToTopButton.module.css';

interface ScrollToTopButtonProps {
  visible: boolean;
  bottom?: number;
  onClick: () => void;
}

export function ScrollToTopButton({
  visible,
  bottom = 12,
  onClick,
}: ScrollToTopButtonProps) {
  return (
    <button
      type="button"
      className={`${styles.button} ${visible ? styles.visible : ''}`}
      style={{ '--scroll-to-top-bottom': `${bottom}px` } as React.CSSProperties}
      onClick={onClick}
      aria-label="Return to Top"
      aria-hidden={!visible}
      tabIndex={visible ? 0 : -1}
    >
      <IconArrowUp size={15} stroke={1.5} />
    </button>
  );
}
