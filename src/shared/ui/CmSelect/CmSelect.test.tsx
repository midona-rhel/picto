import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { CmSelect } from './CmSelect';

describe('CmSelect', () => {
  it('highlights a typed prefix without selecting it until Enter', () => {
    const onChange = vi.fn();
    render(
      <CmSelect
        ariaLabel="Source"
        value="gelbooru"
        options={[
          { value: 'gelbooru', label: 'Gelbooru' },
          { value: 'patreon', label: 'Patreon' },
          { value: 'pixiv', label: 'Pixiv' },
        ]}
        onChange={onChange}
      />,
    );

    const trigger = screen.getByRole('button', { name: 'Source' });
    trigger.focus();
    fireEvent.keyDown(trigger, { key: 'p' });
    fireEvent.keyDown(trigger, { key: 'i' });
    fireEvent.keyDown(trigger, { key: 'x' });

    const listbox = screen.getByRole('listbox', { name: 'Source' });
    const pixiv = screen.getByRole('option', { name: 'Pixiv' });
    expect(listbox).toHaveAttribute('aria-activedescendant', pixiv.id);
    expect(onChange).not.toHaveBeenCalled();
    expect(trigger).toHaveTextContent('Gelbooru');

    fireEvent.keyDown(trigger, { key: 'Enter' });
    expect(onChange).toHaveBeenCalledWith('pixiv');
  });
});
