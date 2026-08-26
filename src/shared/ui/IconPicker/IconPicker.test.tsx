import { fireEvent, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../../test/render';
import { IconPicker } from './IconPicker';

describe('IconPicker', () => {
  it('restores the default icon through the dedicated folder tile', () => {
    const onChange = vi.fn();
    renderWithProviders(
      <IconPicker value="IconPhoto" onChange={onChange} compact />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Use default folder icon' }));

    expect(onChange).toHaveBeenCalledWith(null);
  });

  it('uses the accepted compact presentation explicitly', () => {
    renderWithProviders(<IconPicker value={null} onChange={vi.fn()} compact />);

    expect(document.querySelector('[data-icon-picker-presentation="compact"]')).not.toBeNull();
    expect(screen.getByPlaceholderText('Search icons...').parentElement).toHaveClass(/searchRow/);
  });
});
