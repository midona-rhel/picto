import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { LibrarySwitcherPopover } from './LibrarySwitcherPopover';

const IPC_CANCELLED = "Error invoking remote method 'picto:library:switch': Error: Library open was cancelled";

function renderPopover() {
  return render(
    <LibrarySwitcherPopover anchor={new DOMRect(0, 0, 200, 32)} trigger={null} onClose={() => {}} />,
  );
}

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

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('ignores a cancelled switch and logs any other failed open', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const failures = [new Error(IPC_CANCELLED), new Error('database is locked')];
    const switchLibrary = vi.fn(() => Promise.reject(failures.shift()));
    const open = vi.fn(() => Promise.reject(new Error(IPC_CANCELLED)));
    Object.defineProperty(window, 'picto', {
      configurable: true,
      value: {
        library: {
          getConfig: vi.fn().mockResolvedValue({
            libraryHistory: ['/Pictures/A.library'],
            existsMap: { '/Pictures/A.library': true },
            currentPath: null,
          }),
          switch: switchLibrary,
          open,
        },
      },
    });
    renderPopover();

    fireEvent.click(await screen.findByText('A'));
    await waitFor(() => expect(switchLibrary).toHaveBeenCalledWith('/Pictures/A.library'));
    fireEvent.click(screen.getByText('Open Library…'));
    await waitFor(() => expect(open).toHaveBeenCalledOnce());
    // Both rejections were handled; neither cancellation is an error.
    await Promise.resolve();
    expect(error).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText('A'));
    await waitFor(() => expect(error).toHaveBeenCalledWith(
      '[library] could not switch library',
      expect.objectContaining({ message: 'database is locked' }),
    ));
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
