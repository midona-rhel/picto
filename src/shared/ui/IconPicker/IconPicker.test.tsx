import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { IconPicker } from './IconPicker';

describe('IconPicker', () => {
  it('restores the default icon through the dedicated folder tile', () => {
    const onChange = vi.fn();
    render(
      <div aria-label="Context submenu">
        <IconPicker value="IconPhoto" onChange={onChange} />
      </div>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Use default folder icon' }));

    expect(onChange).toHaveBeenCalledWith(null);
  });
});
