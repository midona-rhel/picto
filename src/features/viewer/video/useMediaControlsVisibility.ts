import { useCallback, useEffect, useRef, useState } from 'react';
import { CONTROLS_HIDE_DELAY } from './videoConstants';

export function useMediaControlsVisibility(holdOpen = false) {
  const [visible, setVisible] = useState(true);
  const timerRef = useRef<ReturnType<typeof setTimeout>>();
  const holdOpenRef = useRef(holdOpen);
  holdOpenRef.current = holdOpen;

  const reveal = useCallback(() => {
    setVisible(true);
    clearTimeout(timerRef.current);
    if (!holdOpenRef.current) {
      timerRef.current = setTimeout(() => setVisible(false), CONTROLS_HIDE_DELAY);
    }
  }, []);

  useEffect(() => {
    if (holdOpen) {
      clearTimeout(timerRef.current);
      setVisible(true);
    } else {
      reveal();
    }
  }, [holdOpen, reveal]);

  useEffect(() => () => clearTimeout(timerRef.current), []);

  return { controlsVisible: visible || holdOpen, revealControls: reveal };
}
