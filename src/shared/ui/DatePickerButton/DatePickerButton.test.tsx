import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { DatePickerButton } from './DatePickerButton';

describe('DatePickerButton', () => {
  it('opens the styled calendar and chooses a date', () => {
    const onChange = vi.fn();
    render(<DatePickerButton value="2026-08-29" onChange={onChange} />);

    fireEvent.click(screen.getByRole('button', { name: 'Choose date' }));
    const calendar = screen.getByRole('dialog', { name: 'Choose date' });
    expect(within(calendar).getByText('August 2026')).toBeVisible();

    fireEvent.click(within(calendar).getByRole('button', { name: 'Next month' }));
    expect(within(calendar).getByText('September 2026')).toBeVisible();
    fireEvent.click(within(calendar).getByText('15', { selector: 'button' }));

    expect(onChange).toHaveBeenCalledWith('2026-09-15');
    expect(screen.queryByRole('dialog', { name: 'Choose date' })).not.toBeInTheDocument();
  });

  it('can clear an existing date', () => {
    const onChange = vi.fn();
    render(<DatePickerButton value="2026-08-29" onChange={onChange} />);
    fireEvent.click(screen.getByRole('button', { name: 'Choose date' }));
    fireEvent.click(within(screen.getByRole('dialog', { name: 'Choose date' })).getByRole('button', { name: 'Clear' }));
    expect(onChange).toHaveBeenCalledWith('');
  });
});
