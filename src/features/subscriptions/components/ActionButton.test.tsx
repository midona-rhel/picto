import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ActionButton } from './ActionButton';
import styles from '../SubscriptionsScreen.module.css';

describe('ActionButton', () => {
  it('blocks repeat clicks without dimming a pending action', () => {
    render(<ActionButton pending>Run now</ActionButton>);

    const button = screen.getByRole('button', { name: 'Run now' });
    expect(button).toBeDisabled();
    expect(button).toHaveClass(styles.buttonPending);
  });
});
