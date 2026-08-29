import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { MantineProvider } from '@mantine/core';
import type { MouseEvent } from 'react';
import { SidebarRow } from './SidebarRow';

describe('SidebarRow', () => {
  it('activates interactive rows from the keyboard', () => {
    const onClick = vi.fn();

    render(
      <SidebarRow
        label="All"
        onClick={onClick}
      />,
    );

    const row = screen.getByRole('button', { name: 'All' });
    fireEvent.keyDown(row, { key: 'Enter' });
    fireEvent.keyDown(row, { key: ' ' });

    expect(onClick).toHaveBeenCalledTimes(2);
  });

  it('toggles section rows from the keyboard', () => {
    const onToggle = vi.fn();

    render(
      <SidebarRow
        variant="section"
        label="Folders"
        count={12}
        expanded
        onToggle={onToggle}
      />,
    );

    const section = screen.getByRole('button', { name: /folders/i });
    fireEvent.keyDown(section, { key: 'Enter' });
    fireEvent.keyDown(section, { key: ' ' });

    expect(onToggle).toHaveBeenCalledTimes(2);
  });

  it('passes the add-button anchor event without toggling the section', () => {
    let addTarget: EventTarget | null = null;
    const onAdd = vi.fn((event: MouseEvent<HTMLButtonElement>) => {
      addTarget = event.currentTarget;
    });
    const onToggle = vi.fn();
    render(
      <MantineProvider>
        <SidebarRow
          variant="section"
          label="Smart Folders"
          expanded
          onToggle={onToggle}
          onAdd={onAdd}
          addTooltip="New Smart Folder or Group"
        />
      </MantineProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'New Smart Folder or Group' }));
    expect(onAdd).toHaveBeenCalledOnce();
    expect(addTarget).toBeInstanceOf(HTMLButtonElement);
    expect(onToggle).not.toHaveBeenCalled();
  });

  it('delegates row indentation to the shared sidebar geometry variable', () => {
    render(
      <SidebarRow
        icon={<span />}
        label="Nested folder"
        indent={2}
        onClick={vi.fn()}
      />,
    );

    const row = screen.getByRole('button', { name: 'Nested folder' });
    expect(row.style.getPropertyValue('--sidebar-row-indent')).toBe('40px');
    expect(row.style.paddingLeft).toBe('');
  });

  it('hides zero and unknown counts while retaining nonzero folder counts', () => {
    const { rerender } = render(
      <SidebarRow variant="folder" icon={<span />} label="Empty folder" count={0} onClick={vi.fn()} />,
    );
    expect(screen.queryByText('0')).not.toBeInTheDocument();

    rerender(
      <SidebarRow variant="folder" icon={<span />} label="Nonempty folder" count={12} onClick={vi.fn()} />,
    );
    expect(screen.getByText('12')).toBeInTheDocument();

    rerender(
      <SidebarRow variant="folder" icon={<span />} label="Loading folder" count={null} onClick={vi.fn()} />,
    );
    expect(screen.queryByText('0')).not.toBeInTheDocument();
  });

  it.each(['folder', 'smart_folder'] as const)('keeps the count in place while %s label is replaced for rename', (variant) => {
    const onClick = vi.fn();
    render(
      <SidebarRow variant={variant} icon={<span />} count={12} onClick={onClick}>
        <input aria-label={`Rename ${variant}`} />
      </SidebarRow>,
    );

    const input = screen.getByRole('textbox', { name: `Rename ${variant}` });
    expect(input).toBeInTheDocument();
    expect(screen.getByText('12')).toBeInTheDocument();
    expect(fireEvent.keyDown(input, { key: ' ' })).toBe(true);
    expect(onClick).not.toHaveBeenCalled();
  });

  it('keeps a no-count system row keyboard-selectable', () => {
    const onClick = vi.fn();
    render(<SidebarRow label="Subscriptions" onClick={onClick} />);
    fireEvent.keyDown(screen.getByRole('button', { name: 'Subscriptions' }), { key: 'Enter' });
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('replaces the count with an accessible subscription pause control', () => {
    const onClick = vi.fn();
    render(
      <MantineProvider>
        <SidebarRow
          label="Subscriptions"
          count={12}
          activity={{ state: 'running', actionLabel: 'Pause all subscriptions', onClick }}
          onClick={vi.fn()}
        />
      </MantineProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Pause all subscriptions' }));
    expect(onClick).toHaveBeenCalledOnce();
    expect(screen.queryByText('12')).not.toBeInTheDocument();
  });

  it('keeps the global paused state visible and exposes resume', () => {
    render(
      <MantineProvider>
        <SidebarRow
          label="Subscriptions"
          activity={{ state: 'paused', actionLabel: 'Resume all subscriptions', onClick: vi.fn() }}
          onClick={vi.fn()}
        />
      </MantineProvider>,
    );

    expect(screen.getByRole('button', { name: 'Resume all subscriptions' })).toBeInTheDocument();
  });
});
