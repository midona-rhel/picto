import { act, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';

const mocks = vi.hoisted(() => ({
  start: vi.fn(() => vi.fn()),
  listen: vi.fn(),
  menuHandler: null as null | (() => void),
}));

vi.mock('../../runtime/updateRuntime', () => ({ startUpdateRuntime: mocks.start }));
vi.mock('../../platform/ipc', () => ({
  listen: mocks.listen.mockImplementation(async (_name: string, handler: () => void) => {
    mocks.menuHandler = handler;
    return vi.fn();
  }),
}));
vi.mock('../modals/UpdateModal', () => ({
  UpdateModal: ({ open }: { open: boolean }) => <div>{open ? 'update-open' : 'update-closed'}</div>,
}));

import { ApplicationUpdateHost } from './ApplicationUpdateHost';

describe('application update host', () => {
  afterEach(() => {
    mocks.menuHandler = null;
    vi.clearAllMocks();
  });

  it('runs and opens updates without requiring a library-bound app shell', async () => {
    renderWithProviders(<ApplicationUpdateHost />);
    expect(mocks.start).toHaveBeenCalledOnce();
    expect(screen.getByText('update-closed')).toBeInTheDocument();
    await waitFor(() => expect(mocks.menuHandler).not.toBeNull());
    act(() => mocks.menuHandler?.());
    expect(screen.getByText('update-open')).toBeInTheDocument();
  });
});
