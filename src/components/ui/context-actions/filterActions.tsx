import type { ContextMenuEntry } from '../ContextMenu';
import type { MimeFilterKey } from '../../../stores/filterStore';
import type { ReactNode } from 'react';

export interface RatingOption {
  label: string;
  value: number | null;
}

export interface MimeOption {
  label: string;
  key: MimeFilterKey;
}

export function buildRatingFilterMenu(
  options: RatingOption[],
  activeRating: number | null,
  onSelect: (value: number | null) => void,
): ContextMenuEntry[] {
  return options.map((opt) => ({
    type: 'check',
    label: opt.label,
    checked: activeRating === opt.value,
    onClick: () => onSelect(opt.value),
  }));
}

export function buildTypesFilterMenu(renderPanel: () => ReactNode): ContextMenuEntry[] {
  return [
    {
      type: 'custom',
      key: 'types-panel',
      render: renderPanel,
    },
  ];
}

export function buildColorFilterMenu(renderPanel: () => ReactNode): ContextMenuEntry[] {
  return [
    {
      type: 'custom',
      key: 'color-panel',
      render: renderPanel,
    },
  ];
}
