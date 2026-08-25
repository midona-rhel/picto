import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { StarRating } from './StarRating';

describe('StarRating', () => {
  it('clears the active rating and prevents duplicate async submissions', async () => {
    let finish: (() => void) | undefined;
    const onChange = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    render(<StarRating value={3} onChange={onChange} />);

    const activeStar = screen.getByRole('button', { name: 'Clear rating' });
    fireEvent.click(activeStar);
    fireEvent.click(activeStar);
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(0);

    finish?.();
    await waitFor(() => expect(activeStar).not.toBeDisabled());
  });

  it('reports a failed rating mutation instead of leaving a rejected promise', async () => {
    const error = new Error('rating failed');
    const onError = vi.fn();
    render(<StarRating value={0} onChange={() => Promise.reject(error)} onError={onError} />);

    fireEvent.click(screen.getByRole('button', { name: 'Set rating to 2 stars' }));
    await waitFor(() => expect(onError).toHaveBeenCalledWith(error));
  });
});
