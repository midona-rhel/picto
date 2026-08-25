import { fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { describe, expect, it, vi } from 'vitest';
import { ColorPalette } from './ColorPalette';

describe('ColorPalette', () => {
  it('filters on left click and opens the shared color action menu on right click', () => {
    const onFilter = vi.fn();
    render(
      <MantineProvider>
        <ColorPalette colors={['#123456']} onFilter={onFilter} />
      </MantineProvider>,
    );

    const swatch = document.querySelector('[class*="swatchWrap"]')!;
    fireEvent.click(swatch);
    expect(onFilter).toHaveBeenCalledWith('#123456');

    fireEvent.contextMenu(swatch, { clientX: 20, clientY: 20 });
    expect(screen.getByRole('menuitem', { name: 'Filter by color' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Copy HEX' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Copy RGB' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Copy RGBA' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Copy HSL' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Copy HSV' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Copy HWB' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Copy CMYK' })).toBeInTheDocument();

    expect(onFilter).toHaveBeenCalledTimes(1);
  });
});
