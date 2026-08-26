import { render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { describe, expect, it } from 'vitest';
import { WindowControls } from './WindowControls';

describe('WindowControls', () => {
  it('renders Picto window controls on Windows', () => {
    render(
      <MantineProvider>
        <WindowControls platform="Win32" />
      </MantineProvider>,
    );

    expect(screen.getByRole('button', { name: 'Minimize' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Maximize' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Close' })).toBeInTheDocument();
  });

  it.each(['MacIntel', 'Linux x86_64'])('uses native controls on %s', (platform) => {
    render(<WindowControls platform={platform} />);

    expect(screen.queryByRole('button', { name: 'Minimize' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Maximize' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Close' })).not.toBeInTheDocument();
  });
});
