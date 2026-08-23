import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ContextMenu } from './ContextMenu';

describe('ContextMenu', () => {
  it('owns one canonical presentation regardless of the supplied entries', () => {
    const { rerender } = render(
      <ContextMenu
        entries={[{ label: 'Sidebar action', action: vi.fn() }]}
        position={{ x: 20, y: 20 }}
        onClose={vi.fn()}
      />,
    );

    const sidebarMenu = screen.getByRole('menu', { name: 'Context menu' });
    const sidebarSearch = screen.getByPlaceholderText('Search...');
    expect(sidebarMenu.className).toContain('menu');
    expect(sidebarMenu).not.toHaveAttribute('style', expect.stringContaining('width'));
    expect(sidebarSearch).toBeInTheDocument();

    rerender(
      <ContextMenu
        entries={[{ label: 'Grid action', action: vi.fn() }]}
        position={{ x: 40, y: 40 }}
        onClose={vi.fn()}
      />,
    );

    const gridMenu = screen.getByRole('menu', { name: 'Context menu' });
    expect(gridMenu.className).toBe(sidebarMenu.className);
    expect(screen.getByPlaceholderText('Search...')).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Grid action' })).toBeInTheDocument();
  });

  it('exposes menu semantics and disabled state', () => {
    render(
      <ContextMenu
        entries={[
          { label: 'Open', action: vi.fn() },
          { label: 'Disabled', action: vi.fn(), disabled: true },
        ]}
        position={{ x: 20, y: 20 }}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByRole('menu', { name: 'Context menu' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Open' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Disabled' })).toHaveAttribute('aria-disabled', 'true');
  });

  it('exposes submenu semantics when opened', () => {
    render(
      <ContextMenu
        entries={[{
          submenu: true,
          label: 'Move to',
          children: [{ label: 'Folder A', action: vi.fn() }],
        }]}
        position={{ x: 20, y: 20 }}
        onClose={vi.fn()}
      />,
    );

    const trigger = screen.getByRole('menuitem', { name: 'Move to' });
    expect(trigger).toHaveAttribute('aria-haspopup', 'menu');
    expect(trigger).toHaveAttribute('aria-expanded', 'false');

    fireEvent.mouseEnter(trigger);
    expect(trigger).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('menu', { name: 'Context submenu' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Folder A' })).toBeInTheDocument();
  });

  it('does not add a second search field to a custom control panel', () => {
    render(
      <ContextMenu
        entries={[{ custom: true, key: 'filters', render: () => <div>Filter controls</div> }]}
        position={{ x: 20, y: 20 }}
        onClose={vi.fn()}
      />,
    );

    expect(screen.queryByPlaceholderText('Search...')).not.toBeInTheDocument();
    expect(screen.getByText('Filter controls')).toBeInTheDocument();
  });
});
