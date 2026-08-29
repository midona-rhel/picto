import { fireEvent, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { InspectorField, InspectorFieldGroup, InspectorSourceField } from './InspectorField';

const openExternalUrl = vi.hoisted(() => vi.fn(() => Promise.resolve()));

vi.mock('../../../controllers/shellController', () => ({
  shellController: { openExternalUrl },
}));

vi.mock('../KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: ReactNode }) => children,
}));

describe('InspectorField', () => {
  beforeEach(() => openExternalUrl.mockClear());

  it('expands overflowing text only after the field is focused', () => {
    const onCommit = vi.fn();
    const value = 'A note long enough to wrap across more than one line in the inspector';
    const view = render(
      <InspectorFieldGroup>
        <InspectorField value={value} placeholder="Notes" onCommit={onCommit} />
      </InspectorFieldGroup>,
    );

    const measure = view.container.querySelector('[data-inspector-field-measure]')!;
    Object.defineProperties(measure, {
      scrollHeight: { configurable: true, value: 52 },
      scrollWidth: { configurable: true, value: 200 },
      clientWidth: { configurable: true, value: 200 },
    });
    view.rerender(
      <InspectorFieldGroup>
        <InspectorField value={`${value}.`} placeholder="Notes" onCommit={onCommit} />
      </InspectorFieldGroup>,
    );

    const field = screen.getByRole('textbox', { name: 'Notes' });
    fireEvent.mouseEnter(field.parentElement!);
    expect(field).not.toHaveAttribute('data-inspector-field-expanded');
    expect(view.container.querySelector('[data-inspector-field-backdrop]')).not.toBeInTheDocument();

    fireEvent.focus(field);
    fireEvent.mouseLeave(field.parentElement!);
    expect(field).toHaveAttribute('data-inspector-field-expanded');
    expect(view.container.querySelectorAll('[role="textbox"]')).toHaveLength(1);
    expect(view.container.querySelector('textarea')).not.toBeInTheDocument();
  });

  it('keeps source rows compact on hover and reveals every URL through the manage action', () => {
    const urls = ['https://example.com/primary/path', 'https://archive.example/another/path'];
    const view = render(
      <InspectorFieldGroup>
        <InspectorSourceField urls={urls} onChange={vi.fn()} />
      </InspectorFieldGroup>,
    );

    fireEvent.click(screen.getByRole('button', { name: `Open ${urls[0]}` }));
    expect(openExternalUrl).toHaveBeenLastCalledWith(urls[0]);

    const wrapper = view.container.firstElementChild!;
    fireEvent.mouseEnter(wrapper);
    expect(view.container.querySelector('[data-inspector-field-backdrop]')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Manage source URLs' }));
    fireEvent.click(screen.getByRole('button', { name: `Open ${urls[1]}` }));
    expect(openExternalUrl).toHaveBeenLastCalledWith(urls[1]);
    expect(view.container.querySelectorAll('[class*="urlRemainder"]')).not.toHaveLength(0);
  });
});
