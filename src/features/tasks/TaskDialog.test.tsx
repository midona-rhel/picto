import { act, fireEvent, screen, within } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import type { ForegroundTask } from '../../shared/types/generated/application/ForegroundTask';
import { applyTasks, minimizeTask, taskPresentationAtom, tasksAtom } from '../../runtime/taskSettle';
import { TaskBar, TaskDialog } from './TaskDialog';

const store = getDefaultStore();

function task(overrides: Partial<ForegroundTask> = {}): ForegroundTask {
  return {
    task_id: 7,
    kind: 'import',
    title: 'Commissions 2024',
    state: 'running',
    scan_complete: true,
    total: 12408,
    done: 4962,
    added: 4651,
    existing: 311,
    failed: 0,
    revertible: true,
    rate_per_second: 42,
    eta_seconds: 180,
    created_at_ms: 1_000,
    finished_at_ms: null,
    error: null,
    ...overrides,
  };
}

let invoke: ReturnType<typeof vi.fn>;

beforeEach(() => {
  store.set(tasksAtom, []);
  store.set(taskPresentationAtom, { focusedTaskId: null, minimized: new Set<number>() });
  invoke = vi.fn().mockResolvedValue([]);
  Object.defineProperty(window, 'picto', { configurable: true, value: { api: { invoke } } });
});

afterEach(() => {
  vi.restoreAllMocks();
});

function show(next: ForegroundTask) {
  act(() => applyTasks([next]));
}

describe('TaskDialog', () => {
  it('takes the foreground for a working import with progress and an estimate', () => {
    renderWithProviders(<><TaskDialog /><TaskBar /></>);
    show(task());

    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByText('Importing “Commissions 2024”')).toBeInTheDocument();
    expect(within(dialog).getByText('Adding to library · 42 files/s')).toBeInTheDocument();
    expect(within(dialog).getByText(`${(4962).toLocaleString()} of ${(12408).toLocaleString()} files`)).toBeInTheDocument();
    expect(within(dialog).getByText('About 3 min left')).toBeInTheDocument();
    expect(within(dialog).getByText('311 already in library')).toBeInTheDocument();
    expect(within(dialog).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '40');
  });

  it('shows the scan before the total is known', () => {
    renderWithProviders(<TaskDialog />);
    show(task({ scan_complete: false, total: 900, done: 0, eta_seconds: null, rate_per_second: null }));

    expect(screen.getByText('Finding files…')).toBeInTheDocument();
    expect(screen.getByText('900 files found')).toBeInTheDocument();
    expect(screen.getByRole('progressbar')).not.toHaveAttribute('aria-valuenow');
  });

  it('pauses and resumes through the backend', async () => {
    renderWithProviders(<TaskDialog />);
    show(task());

    invoke.mockResolvedValueOnce([task({ state: 'paused', eta_seconds: null })]);
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Pause' })); });
    expect(invoke).toHaveBeenCalledWith('tasks.pause', { task_id: 7 });
  });

  it('pauses while the user chooses how to stop, and can remove what was imported', async () => {
    renderWithProviders(<TaskDialog />);
    show(task());
    invoke.mockResolvedValue([task({ state: 'paused' })]);

    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Stop…' })); });
    expect(invoke).toHaveBeenCalledWith('tasks.pause', { task_id: 7 });
    expect(screen.getByText('Stop importing “Commissions 2024”?')).toBeInTheDocument();
    const keep = screen.getByRole('radio', { name: /Keep what was imported/ });
    expect(keep).toHaveAttribute('aria-checked', 'true');

    fireEvent.click(screen.getByRole('radio', { name: /Remove what was imported/ }));
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Stop and remove' })); });
    expect(invoke).toHaveBeenCalledWith('tasks.stop', { task_id: 7, undo: true });
  });

  it('resumes when the user decides to keep importing', async () => {
    renderWithProviders(<TaskDialog />);
    show(task());
    invoke.mockResolvedValue([task({ state: 'paused' })]);

    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Stop…' })); });
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Keep importing' })); });
    expect(invoke).toHaveBeenCalledWith('tasks.resume', { task_id: 7 });
  });

  it('offers no removal for an import that moved its files', async () => {
    renderWithProviders(<TaskDialog />);
    show(task({ state: 'paused', revertible: false }));

    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Stop…' })); });
    expect(screen.queryByRole('radio')).not.toBeInTheDocument();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Stop import' })); });
    expect(invoke).toHaveBeenCalledWith('tasks.stop', { task_id: 7, undo: false });
    expect(invoke).not.toHaveBeenCalledWith('tasks.pause', expect.anything());
  });

  it('summarises a finished import and lists the files that failed', async () => {
    renderWithProviders(<TaskDialog />);
    act(() => {
      store.set(taskPresentationAtom, { focusedTaskId: 7, minimized: new Set<number>() });
      applyTasks([task({ state: 'finished', done: 12408, added: 12095, failed: 2, finished_at_ms: 291_000 })]);
    });

    expect(screen.getByText('Imported “Commissions 2024”')).toBeInTheDocument();
    expect(screen.getByText('Finished in 4 min 50 s')).toBeInTheDocument();
    invoke.mockResolvedValueOnce([{ path: '/in/z-empty.png', error: 'Zero-size file' }]);
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Show 2 failed' })); });
    expect(invoke).toHaveBeenCalledWith('tasks.failures', { task_id: 7 });
    expect(await screen.findByText('z-empty.png')).toBeInTheDocument();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Done' })); });
    expect(invoke).toHaveBeenCalledWith('tasks.dismiss', { task_id: 7 });
  });

  it('runs in the background as a bar row and comes back when opened', () => {
    renderWithProviders(<><TaskDialog /><TaskBar /></>);
    show(task());

    fireEvent.click(screen.getByRole('button', { name: 'Run in background' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    const row = screen.getByRole('status');
    expect(within(row).getByText(`${(4962).toLocaleString()} / ${(12408).toLocaleString()} · 3 min`)).toBeInTheDocument();

    fireEvent.click(within(row).getByRole('button', { name: 'Show' }));
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });

  it('does not pull a backgrounded task back into the foreground on refresh', () => {
    renderWithProviders(<TaskDialog />);
    show(task());
    act(() => minimizeTask(7));
    show(task({ done: 5000 }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
});
