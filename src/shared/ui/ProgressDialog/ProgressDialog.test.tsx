import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ProgressDialog } from './ProgressDialog';

describe('ProgressDialog', () => {
  it('blocks the window and reports determinate progress', () => {
    render(
      <ProgressDialog
        open
        message="Importing library"
        detail="4 / 10"
        done={4}
        total={10}
      />,
    );

    expect(screen.getByRole('dialog', { name: 'Importing library' })).toHaveAttribute('aria-modal', 'true');
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '40');
  });

  it('does not mount when closed', () => {
    render(<ProgressDialog open={false} message="Importing library" />);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
});
