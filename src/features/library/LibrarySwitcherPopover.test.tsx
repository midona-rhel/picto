import { fireEvent, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LibrarySwitcherPopover } from './LibrarySwitcherPopover';

describe('LibrarySwitcherPopover', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'picto', {
      configurable: true,
      value: {
        library: {
          getConfig: vi.fn(() => new Promise(() => {})),
        },
      },
    });
  });

  it('does not treat a second click on its trigger as an outside click', () => {
    const trigger = document.createElement('button');
    document.body.appendChild(trigger);
    const onClose = vi.fn();

    render(
      <LibrarySwitcherPopover
        anchor={new DOMRect(0, 0, 200, 32)}
        trigger={trigger}
        onClose={onClose}
      />,
    );

    fireEvent.mouseDown(trigger);
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.mouseDown(document.body);
    expect(onClose).toHaveBeenCalledOnce();
    trigger.remove();
  });
});
