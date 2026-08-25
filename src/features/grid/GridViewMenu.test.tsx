import { fireEvent, render, screen } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it } from 'vitest';
import { gridGrayscaleAtom } from '../../state/grid';
import type { MenuCustom } from '../../shared/ui/ContextMenu/ContextMenu';
import { buildViewMenuEntries } from './GridViewMenu';

const store = getDefaultStore();

describe('GridViewMenu', () => {
  beforeEach(() => {
    store.set(gridGrayscaleAtom, false);
  });

  it('toggles grayscale for the grid session without persisting a library preference', () => {
    const display = buildViewMenuEntries().find(
      (entry): entry is MenuCustom => 'custom' in entry && entry.key === 'display-toggles',
    );
    expect(display).toBeDefined();
    render(display!.render());

    fireEvent.click(screen.getByText('Grayscale Preview'));

    expect(store.get(gridGrayscaleAtom)).toBe(true);
  });
});
