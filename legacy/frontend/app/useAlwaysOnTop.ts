import { useCallback, useState } from 'react';
import { getCurrentWindow } from '#desktop/api';

export function useAlwaysOnTop() {
  const [alwaysOnTop, setAlwaysOnTop] = useState(false);
  const toggleAlwaysOnTop = useCallback(() => {
    const next = !alwaysOnTop;
    setAlwaysOnTop(next);
    getCurrentWindow().setAlwaysOnTop(next).catch(() => {});
  }, [alwaysOnTop]);
  return { alwaysOnTop, toggleAlwaysOnTop };
}
