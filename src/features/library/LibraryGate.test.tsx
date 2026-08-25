import { act, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { LibraryGate } from './LibraryGate';

type EventHandler = (payload: unknown) => void;

function installDesktop(currentPath: string | null) {
  const handlers = new Map<string, EventHandler>();
  const invoke = vi.fn().mockResolvedValue(null);
  Object.defineProperty(window, 'picto', {
    configurable: true,
    value: {
      api: { invoke },
      events: {
        on: vi.fn((name: string, handler: EventHandler) => {
          handlers.set(name, handler);
          return Promise.resolve(() => handlers.delete(name));
        }),
      },
      library: {
        getConfig: vi.fn().mockResolvedValue({ currentPath }),
      },
      window: { call: vi.fn().mockResolvedValue(null) },
    },
  });
  return { handlers, invoke };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('LibraryGate', () => {
  it('does not mount library-backed UI and opens the chooser when no library exists', async () => {
    const { invoke } = installDesktop(null);
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);

    expect(await screen.findByText('Open a library to start')).toBeInTheDocument();
    expect(screen.queryByText('Library content')).not.toBeInTheDocument();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('open_library_manager', {}));
  });

  it('mounts library-backed UI after the host reports a successful switch', async () => {
    const { handlers } = installDesktop(null);
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);
    await screen.findByText('Open a library to start');

    act(() => handlers.get('library-switched')?.({ path: '/tmp/Main.library' }));

    expect(await screen.findByText('Library content')).toBeInTheDocument();
    expect(screen.queryByText('Open a library to start')).not.toBeInTheDocument();
  });

  it('mounts library-backed UI immediately when a library is already open', async () => {
    const { invoke } = installDesktop('/tmp/Main.library');
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);

    expect(await screen.findByText('Library content')).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith('open_library_manager', {});
  });
});
