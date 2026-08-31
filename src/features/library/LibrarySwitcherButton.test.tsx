import { act, render } from '@testing-library/react';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  getConfig: vi.fn(),
}));

vi.mock('../../platform/ipc', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock('../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: ReactNode }) => children,
}));

vi.mock('./LibraryAvatar', () => ({
  LibraryAvatar: () => <span data-testid="library-avatar" />,
}));

vi.mock('./LibrarySwitcherPopover', () => ({
  LibrarySwitcherPopover: () => null,
}));

import { LibrarySwitcherButton } from './LibrarySwitcherButton';

describe('LibrarySwitcherButton', () => {
  beforeEach(() => {
    mocks.getConfig.mockReset();
    (window as any).picto = {
      ...(window as any).picto,
      library: { getConfig: mocks.getConfig },
    };
  });

  it('reserves the same switcher element while library metadata loads', async () => {
    let resolveConfig: ((value: unknown) => void) | undefined;
    mocks.getConfig.mockReturnValue(new Promise((resolve) => { resolveConfig = resolve; }));
    const view = render(<LibrarySwitcherButton />);
    const initialButton = view.container.querySelector('button');

    expect(initialButton).not.toBeNull();
    expect(initialButton).toHaveAttribute('data-loading', 'true');

    await act(async () => {
      resolveConfig?.({ currentPath: '/Pictures/Main.library', libraryMeta: {} });
    });

    expect(view.container.querySelector('button')).toBe(initialButton);
    expect(initialButton).not.toHaveAttribute('data-loading');
    expect(initialButton).toHaveTextContent('Main');
  });
});
