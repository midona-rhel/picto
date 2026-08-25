import { afterEach, describe, expect, it, vi } from 'vitest';

const runtime = vi.hoisted(() => ({
  refreshSubscriptionsWorkspace: vi.fn().mockResolvedValue(undefined),
  startSubscriptionsSettle: vi.fn(() => vi.fn()),
  libraryStart: vi.fn(),
  libraryStop: vi.fn(),
}));

vi.mock('./subscriptionsSettle', () => ({
  refreshSubscriptionsWorkspace: runtime.refreshSubscriptionsWorkspace,
  startSubscriptionsSettle: runtime.startSubscriptionsSettle,
}));
vi.mock('./libraryInvalidation', () => ({
  libraryInvalidation: { start: runtime.libraryStart, stop: runtime.libraryStop },
}));
vi.mock('./appSettingsSettle', () => ({ startAppSettingsSettle: () => vi.fn() }));
vi.mock('./applicationMenuRuntime', () => ({ startApplicationMenuRuntime: () => vi.fn() }));
vi.mock('./diagnosticsRuntime', () => ({ startDiagnosticsRuntime: () => vi.fn() }));
vi.mock('./gridSettle', () => ({ startGridSettle: () => vi.fn() }));
vi.mock('./historyRuntime', () => ({ startHistoryRuntime: () => vi.fn() }));
vi.mock('./inspectorSettle', () => ({ startInspectorSettle: () => vi.fn() }));
vi.mock('./sidebarSettle', () => ({ startSidebarSettle: () => vi.fn() }));

import { startAppRuntime } from './appRuntime';

describe('application runtime', () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it('preloads and keeps subscription state settled before navigation', () => {
    const stop = startAppRuntime();

    expect(runtime.refreshSubscriptionsWorkspace).toHaveBeenCalledOnce();
    expect(runtime.startSubscriptionsSettle).toHaveBeenCalledOnce();
    stop();
    expect(runtime.libraryStop).toHaveBeenCalledOnce();
  });
});
