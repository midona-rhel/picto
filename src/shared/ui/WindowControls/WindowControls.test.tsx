import { fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { describe, expect, it, vi } from 'vitest';
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

  it('uses native controls on macOS', () => {
    const platform = 'MacIntel';
    render(<WindowControls platform={platform} />);

    expect(screen.queryByRole('button', { name: 'Minimize' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Maximize' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Close' })).not.toBeInTheDocument();
  });

  it('renders Picto window controls on Linux', () => {
    render(<MantineProvider><WindowControls platform="Linux x86_64" /></MantineProvider>);
    expect(screen.getByRole('button', { name: 'Close' })).toBeInTheDocument();
  });

  it('routes every control through the desktop window API', () => {
    const call = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(window, 'picto', {
      value: { api: { window: { call } } },
      configurable: true,
    });
    render(<MantineProvider><WindowControls platform="Win32" /></MantineProvider>);
    fireEvent.click(screen.getByRole('button', { name: 'Minimize' }));
    fireEvent.click(screen.getByRole('button', { name: 'Maximize' }));
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(call.mock.calls).toEqual([
      ['minimize'],
      ['toggleMaximize'],
      ['close'],
    ]);
  });
});
