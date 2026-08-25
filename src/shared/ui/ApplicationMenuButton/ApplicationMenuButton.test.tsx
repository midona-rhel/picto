import { fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { describe, expect, it, vi } from 'vitest';
import { ApplicationMenuButton, usesInWindowApplicationMenu } from './ApplicationMenuButton';

describe('ApplicationMenuButton', () => {
  it('uses the in-window menu on Windows/Linux and only debug macOS', () => {
    expect(usesInWindowApplicationMenu('Win32', false)).toBe(true);
    expect(usesInWindowApplicationMenu('Linux x86_64', false)).toBe(true);
    expect(usesInWindowApplicationMenu('MacIntel', false)).toBe(false);
    expect(usesInWindowApplicationMenu('MacIntel', true)).toBe(true);
  });

  it('opens the shared Picto menu and executes its native command model', async () => {
    const executeApplicationMenuItem = vi.fn();
    (window as any).picto = { api: {
      getApplicationMenu: vi.fn().mockResolvedValue([{
        id: '1', label: 'File', type: 'submenu', enabled: true, checked: false, accelerator: null,
        submenu: [{
          id: '1.0', label: 'Import Files…', type: 'normal', enabled: true, checked: false,
          accelerator: 'CmdOrCtrl+I', submenu: null,
        }],
      }]),
      executeApplicationMenuItem,
    } };

    render(
      <MantineProvider>
        <ApplicationMenuButton platform="Win32" debug={false} />
      </MantineProvider>,
    );

    const button = screen.getByRole('button', { name: 'Application menu' });
    expect(button).toHaveAttribute('aria-haspopup', 'menu');
    fireEvent.click(button);
    const fileMenu = await screen.findByRole('menuitem', { name: /File/ });
    expect(document.querySelector('[data-menu-icon-slot]')).not.toBeInTheDocument();
    fireEvent.click(fileMenu);
    const importFiles = await screen.findByRole('menuitem', { name: /Import Files/ });
    expect(importFiles).not.toHaveTextContent('CmdOrCtrl');
    fireEvent.click(importFiles);
    expect(executeApplicationMenuItem).toHaveBeenCalledWith('1.0');

    delete (window as any).picto;
  });
});
