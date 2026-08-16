import { fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { describe, expect, it, vi } from 'vitest';
import { ApplicationMenuButton, usesInWindowApplicationMenu } from './ApplicationMenuButton';

describe('ApplicationMenuButton', () => {
  it('is limited to Windows and Linux, leaving macOS to its native menu bar', () => {
    expect(usesInWindowApplicationMenu('Win32')).toBe(true);
    expect(usesInWindowApplicationMenu('Linux x86_64')).toBe(true);
    expect(usesInWindowApplicationMenu('MacIntel')).toBe(false);
  });

  it('opens the existing native application menu with button semantics', () => {
    const popupMenu = vi.fn();
    (window as any).picto = { popupMenu };

    render(
      <MantineProvider>
        <ApplicationMenuButton />
      </MantineProvider>,
    );

    const button = screen.getByRole('button', { name: 'Application menu' });
    expect(button).toHaveAttribute('aria-haspopup', 'menu');
    fireEvent.click(button);
    expect(popupMenu).toHaveBeenCalledOnce();

    delete (window as any).picto;
  });
});
