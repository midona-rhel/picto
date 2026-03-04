import { useState, useCallback, useRef } from 'react';

/**
 * Provides navigate + boundary flash logic for image viewers.
 * Shows a "First item" / "Last item" indicator when the user
 * tries to navigate past the edges.
 *
 * The returned `navigate` is identity-stable (never recreated)
 * so keyboard effects don't need it in their dependency arrays.
 */
export function useBoundaryNavigation(
  totalItems: number,
  onNavigate: (newIndex: number) => void,
  currentIndexRef: React.MutableRefObject<number>,
): {
  navigate: (direction: number) => void;
  boundaryFlash: 'left' | 'right' | null;
} {
  const [boundaryFlash, setBoundaryFlash] = useState<'left' | 'right' | null>(null);
  const boundaryTimerRef = useRef<ReturnType<typeof setTimeout>>();

  // Store mutable values in refs so navigate never needs to be recreated
  const totalItemsRef = useRef(totalItems);
  totalItemsRef.current = totalItems;
  const onNavigateRef = useRef(onNavigate);
  onNavigateRef.current = onNavigate;

  const navigate = useCallback((direction: number) => {
    const newIndex = currentIndexRef.current + direction;
    if (newIndex < 0) {
      setBoundaryFlash('left');
      clearTimeout(boundaryTimerRef.current);
      boundaryTimerRef.current = setTimeout(() => setBoundaryFlash(null), 800);
      return;
    }
    if (newIndex >= totalItemsRef.current) {
      setBoundaryFlash('right');
      clearTimeout(boundaryTimerRef.current);
      boundaryTimerRef.current = setTimeout(() => setBoundaryFlash(null), 800);
      return;
    }
    onNavigateRef.current(newIndex);
  }, [currentIndexRef]); // eslint-disable-line react-hooks/exhaustive-deps

  return { navigate, boundaryFlash };
}
