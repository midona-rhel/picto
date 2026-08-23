import type { LayoutResult } from './layout/types';

export type GridScrollAlignment = 'nearest' | 'center';

export function scrollGridItemIntoView(
  container: HTMLDivElement,
  layout: LayoutResult,
  index: number,
  alignment: GridScrollAlignment = 'nearest',
): number | null {
  const position = layout.positions[index];
  if (!position) return null;

  const viewportHeight = container.clientHeight;
  const current = container.scrollTop;
  const contentOffset = container.querySelector<HTMLElement>('[data-grid-layout]')?.offsetTop ?? 0;
  const itemTop = contentOffset + position.y;
  let next = current;

  if (alignment === 'center') {
    next = itemTop + position.h / 2 - viewportHeight / 2;
  } else if (itemTop < current + 16) {
    next = itemTop - 16;
  } else if (itemTop + position.h > current + viewportHeight - 16) {
    next = itemTop + position.h - viewportHeight + 16;
  }

  container.scrollTop = Math.max(0, next);
  return container.scrollTop;
}
