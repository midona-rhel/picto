import { fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { describe, expect, it, vi } from 'vitest';
import { FilterLogicTabs } from './FilterLogicTabs';

describe('FilterLogicTabs', () => {
  it('exposes any, all, and exact matching rules', () => {
    const onChange = vi.fn();
    render(<MantineProvider><FilterLogicTabs value="any" onChange={onChange} /></MantineProvider>);

    expect(screen.getByRole('button', { name: 'Match any' }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('button', { name: 'Match any' }).querySelector('.tabler-icon-layers-union')).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Match all' }).querySelector('.tabler-icon-layers-intersect')).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Match all' }));
    fireEvent.click(screen.getByRole('button', { name: 'Match exactly' }));

    expect(onChange).toHaveBeenNthCalledWith(1, 'all');
    expect(onChange).toHaveBeenNthCalledWith(2, 'exact');
  });
});
