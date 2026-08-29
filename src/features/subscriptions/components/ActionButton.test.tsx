import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ActionButton } from './ActionButton';
import styles from '../SubscriptionsScreen.module.css';

describe('ActionButton', () => {
  it('blocks repeat clicks without changing the painted button state', async () => {
    const onClick = vi.fn();
    const user = userEvent.setup();
    render(<ActionButton pending onClick={onClick}>Run now</ActionButton>);

    const button = screen.getByRole('button', { name: 'Run now' });
    expect(button).not.toBeDisabled();
    expect(button).toHaveAttribute('aria-disabled', 'true');
    expect(button).toHaveAttribute('aria-busy', 'true');
    expect(button).toHaveClass(styles.buttonPending);
    await user.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });
});
