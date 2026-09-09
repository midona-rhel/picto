import { act, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { LibraryGate } from './LibraryGate';

type EventHandler = (payload: unknown) => void;

function installDesktop(currentPath: string | null, config: Record<string, unknown> = {}) {
  const handlers = new Map<string, EventHandler>();
  const invoke = vi.fn().mockResolvedValue(null);
  const getConfig = vi.fn().mockResolvedValue({ currentPath, ...config });
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
        getConfig,
      },
      window: { call: vi.fn().mockResolvedValue(null) },
    },
  });
  return { handlers, invoke, getConfig };
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

  it('returns to library selection when the open library becomes unreadable', async () => {
    const { handlers, invoke } = installDesktop('/tmp/Main.library');
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);
    expect(await screen.findByText('Library content')).toBeInTheDocument();

    act(() => handlers.get('library-open-failed')?.({
      path: '/tmp/Main.library',
      message: 'database is unreadable',
    }));

    expect(await screen.findByText('Open a library to start')).toBeInTheDocument();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('open_library_manager', {}));
  });

  it('reports an unreadable library configuration before opening the chooser', async () => {
    const { getConfig, invoke } = installDesktop('/tmp/Main.library');
    getConfig.mockRejectedValue(new Error('configuration read failed'));

    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);

    expect(await screen.findByText('Open a library to start')).toBeInTheDocument();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      'library.initial_read_failed',
      { message: 'configuration read failed' },
    ));
  });

  it('shows the migration stage reported by library.open_status while opening', async () => {
    const { invoke } = installDesktop(null, { openingPath: '/tmp/Main.library' });
    invoke.mockImplementation(async (command: string) => (
      command === 'library.open_status' ? { path: '/tmp/Main.library', stage: 'migrating' } : null
    ));
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);

    expect(await screen.findByText('Updating your library database')).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith('library.open_status', {});
    expect(invoke).not.toHaveBeenCalledWith('cloud.status.get', {});
    expect(screen.queryByText('Library content')).not.toBeInTheDocument();
  });

  it('keeps the generic headline while library.open_status reports nothing in flight', async () => {
    const { invoke } = installDesktop(null, { openingPath: '/tmp/Main.library' });
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);

    expect(await screen.findByText('Opening your library')).toBeInTheDocument();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('library.open_status', {}));
    expect(screen.getByText('Opening your library')).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith('open_library_manager', {});
  });

  it('mounts library-backed UI once the pending open reports library-switched', async () => {
    const { handlers } = installDesktop(null, { openingPath: '/tmp/Main.library' });
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);
    await screen.findByText('Opening your library');

    act(() => handlers.get('library-switched')?.({ path: '/tmp/Main.library' }));

    expect(await screen.findByText('Library content')).toBeInTheDocument();
    expect(screen.queryByText('Opening your library')).not.toBeInTheDocument();
  });

  it('returns to library selection when the pending open is cancelled', async () => {
    const { handlers, invoke } = installDesktop(null, { openingPath: '/tmp/Main.library' });
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);
    await screen.findByText('Opening your library');

    act(() => handlers.get('library-open-failed')?.({
      path: '/tmp/Main.library',
      message: 'Library open was cancelled',
      cancelled: true,
    }));

    expect(await screen.findByText('Open a library to start')).toBeInTheDocument();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('open_library_manager', {}));
  });

  it('keeps the last reported stage after library.open_status reports nothing in flight', async () => {
    const { invoke } = installDesktop(null, { openingPath: '/tmp/Main.library' });
    let polls = 0;
    invoke.mockImplementation(async (command: string) => {
      if (command !== 'library.open_status') return null;
      polls += 1;
      return polls === 1 ? { path: '/tmp/Main.library', stage: 'migrating' } : null;
    });
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);

    expect(await screen.findByText('Updating your library database')).toBeInTheDocument();
    await waitFor(() => expect(polls).toBeGreaterThanOrEqual(3));
    expect(screen.getByText('Updating your library database')).toBeInTheDocument();
  });

  it('drops the stage of a superseded open when a different library starts opening', async () => {
    const { handlers, invoke } = installDesktop(null, { openingPath: '/tmp/A.library' });
    let status: { path: string; stage: string } = { path: '/tmp/A.library', stage: 'migrating' };
    let polls = 0;
    invoke.mockImplementation(async (command: string) => {
      if (command !== 'library.open_status') return null;
      polls += 1;
      return status;
    });
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);
    expect(await screen.findByText('Updating your library database')).toBeInTheDocument();

    act(() => handlers.get('library-switching')?.({ path: '/tmp/B.library' }));

    // The stage of A is gone in the very render that shows B opening, and
    // the host still reporting the superseded open of A does not bring it back.
    expect(screen.getByText('Opening your library')).toBeInTheDocument();
    const pollsAfterSwitch = polls;
    await waitFor(() => expect(polls).toBeGreaterThanOrEqual(pollsAfterSwitch + 2));
    expect(screen.getByText('Opening your library')).toBeInTheDocument();
    expect(screen.queryByText('Updating your library database')).not.toBeInTheDocument();

    status = { path: '/tmp/B.library', stage: 'loading' };
    expect(await screen.findByText('Loading your library')).toBeInTheDocument();
  });

  it('shows the generic headline when a library that was opening starts opening again', async () => {
    const { handlers, invoke } = installDesktop(null, { openingPath: '/tmp/A.library' });
    let status: { path: string; stage: string } | null = { path: '/tmp/A.library', stage: 'starting' };
    let polls = 0;
    invoke.mockImplementation(async (command: string) => {
      if (command !== 'library.open_status') return null;
      polls += 1;
      return status;
    });
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);
    expect(await screen.findByText('Starting library services')).toBeInTheDocument();

    // The host has nothing to report yet for B, nor for A opening again.
    status = null;
    act(() => handlers.get('library-switching')?.({ path: '/tmp/B.library' }));
    expect(screen.getByText('Opening your library')).toBeInTheDocument();
    act(() => handlers.get('library-switching')?.({ path: '/tmp/A.library' }));

    // Where A's previous open got to must not come back before A's new
    // open reports a stage of its own.
    expect(screen.getByText('Opening your library')).toBeInTheDocument();
    const pollsAfterSwitch = polls;
    await waitFor(() => expect(polls).toBeGreaterThanOrEqual(pollsAfterSwitch + 2));
    expect(screen.getByText('Opening your library')).toBeInTheDocument();
    expect(screen.queryByText('Starting library services')).not.toBeInTheDocument();

    status = { path: '/tmp/A.library', stage: 'loading' };
    expect(await screen.findByText('Loading your library')).toBeInTheDocument();
  });

  it('shows the generic headline when a failed open of the same library is retried', async () => {
    const { handlers, invoke } = installDesktop(null, { openingPath: '/tmp/A.library' });
    let status: { path: string; stage: string } | null = { path: '/tmp/A.library', stage: 'starting' };
    invoke.mockImplementation(async (command: string) => (
      command === 'library.open_status' ? status : null
    ));
    renderWithProviders(<LibraryGate><div>Library content</div></LibraryGate>);
    expect(await screen.findByText('Starting library services')).toBeInTheDocument();

    status = null;
    act(() => handlers.get('library-open-failed')?.({ path: '/tmp/A.library', message: 'database is locked' }));
    expect(await screen.findByText('Open a library to start')).toBeInTheDocument();
    act(() => handlers.get('library-switching')?.({ path: '/tmp/A.library' }));

    expect(screen.getByText('Opening your library')).toBeInTheDocument();
    expect(screen.queryByText('Starting library services')).not.toBeInTheDocument();
  });
});
