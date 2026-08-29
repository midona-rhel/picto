import { act, fireEvent, render, renderHook, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ContextMenu, searchMenuEntries, useContextMenu } from './ContextMenu';

describe('ContextMenu', () => {
  it('ignores a deferred entry update after a newer menu opens', () => {
    const { result } = renderHook(() => useContextMenu());
    let staleId = 0;
    act(() => {
      staleId = result.current.openAt({ x: 10, y: 10 }, [{ label: 'Open With Other', action: vi.fn() }]);
      result.current.openAt({ x: 20, y: 20 }, [{ label: 'Current action', action: vi.fn() }]);
      result.current.replaceEntry(staleId, 'Open With Other', [{ label: 'Preview', action: vi.fn() }]);
    });

    expect(result.current.state?.entries).toHaveLength(1);
    expect(result.current.state?.entries[0]).toMatchObject({ label: 'Current action' });
  });

  it('owns one canonical presentation regardless of the supplied entries', () => {
    const sidebarEntries = Array.from({ length: 7 }, (_, index) => ({
      label: `Sidebar action ${index + 1}`,
      action: vi.fn(),
    }));
    const gridEntries = Array.from({ length: 7 }, (_, index) => ({
      label: `Grid action ${index + 1}`,
      action: vi.fn(),
    }));
    const { rerender } = render(
      <ContextMenu
        entries={sidebarEntries}
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
        entries={gridEntries}
        position={{ x: 40, y: 40 }}
        onClose={vi.fn()}
      />,
    );

    const gridMenu = screen.getByRole('menu', { name: 'Context menu' });
    expect(gridMenu.className).toBe(sidebarMenu.className);
    expect(screen.getByPlaceholderText('Search...')).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Grid action 1' })).toBeInTheDocument();
  });

  it('shows search only for menus with at least seven items', () => {
    const entries = Array.from({ length: 7 }, (_, index) => ({
      label: `Action ${index + 1}`,
      action: vi.fn(),
    }));
    const { rerender } = render(
      <ContextMenu
        entries={entries.slice(0, 6)}
        position={{ x: 20, y: 20 }}
        onClose={vi.fn()}
      />,
    );

    expect(screen.queryByPlaceholderText('Search...')).not.toBeInTheDocument();

    rerender(
      <ContextMenu
        entries={entries}
        position={{ x: 20, y: 20 }}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByPlaceholderText('Search...')).toBeInTheDocument();
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

  it('does not reserve an icon column when a menu level has no icons', () => {
    render(
      <ContextMenu
        entries={[{
          submenu: true,
          label: 'Set Rating',
          children: [{ label: 'No Rating', action: vi.fn() }],
        }]}
        position={{ x: 20, y: 20 }}
        onClose={vi.fn()}
      />,
    );

    expect(document.querySelector('[data-menu-icon-slot]')).not.toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByRole('menuitem', { name: 'Set Rating' }));
    expect(screen.getByRole('menuitem', { name: 'No Rating' }).querySelector('[data-menu-icon-slot]')).toBeNull();
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

  it('supports content-specific width without changing the shared chrome', () => {
    render(
      <ContextMenu
        entries={[{ custom: true, key: 'filters', render: () => <div>Filter controls</div> }]}
        position={{ x: 20, y: 20 }}
        onClose={vi.fn()}
        width={220}
      />,
    );

    const menu = screen.getByRole('menu', { name: 'Context menu' });
    expect(menu).toHaveStyle({ width: '220px' });
    expect(menu.className).toContain('menu');
  });

  it('supports facet include on click and exclude on right-click', () => {
    const include = vi.fn();
    const exclude = vi.fn();
    render(
      <ContextMenu
        entries={[{ label: 'JPG', action: include, contextAction: exclude, keepOpen: true }]}
        position={{ x: 20, y: 20 }}
        onClose={vi.fn()}
      />,
    );

    const item = screen.getByRole('menuitem', { name: 'JPG' });
    fireEvent.click(item);
    expect(include).toHaveBeenCalledOnce();
    fireEvent.contextMenu(item);
    expect(exclude).toHaveBeenCalledOnce();
    expect(screen.getByRole('menu', { name: 'Context menu' })).toBeInTheDocument();
  });
});

describe('searchMenuEntries', () => {
  it('finds alternate keywords and executable submenu actions', () => {
    const exportAction = vi.fn();
    const entries = [
      {
        submenu: true as const,
        label: 'Organize',
        children: [
          { label: 'Export Originals', keywords: 'save files', action: exportAction },
        ],
      },
    ];

    expect(searchMenuEntries(entries, 'save')).toEqual([entries[0].children[0]]);
    expect(searchMenuEntries(entries, 'original')).toEqual([entries[0].children[0]]);
  });

  it('ranks a visible label match before a keyword match', () => {
    const entries = [
      { label: 'Reveal in Finder', keywords: 'open folder', action: vi.fn() },
      { label: 'Open With', keywords: 'reveal', action: vi.fn() },
    ];

    expect(searchMenuEntries(entries, 'reveal')).toEqual([entries[0], entries[1]]);
  });
});
