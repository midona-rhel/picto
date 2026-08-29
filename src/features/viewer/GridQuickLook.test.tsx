import { act, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { GridQuickLook } from './GridQuickLook';

vi.mock('../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: ReactNode }) => children,
}));

vi.mock('../groups/GroupQuickLook', () => ({
  GroupQuickLookContent: () => <div data-testid="group-content" />,
}));

vi.mock('./QuickLook', () => ({
  QuickLookContent: ({ currentIndex }: { currentIndex: number }) => (
    <div data-testid="media-content" data-item-index={currentIndex} />
  ),
}));

const items = [
  { root_id: 7, kind: 'collection', content_hash: 'group', mime: 'application/x-collection' },
  { root_id: 8, kind: 'media', content_hash: 'media', mime: 'image/jpeg' },
  { root_id: 9, kind: 'media', content_hash: 'media-next', mime: 'image/png' },
] as never;

describe('GridQuickLook', () => {
  it('keeps the decoded group visible until the adjacent media thumbnail is decoded', async () => {
    let resolveDecode: (() => void) | null = null;
    class ControlledImage {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      decode = () => new Promise<void>((resolve) => { resolveDecode = resolve; });
      set src(_value: string) { this.onload?.(); }
    }
    vi.stubGlobal('Image', ControlledImage);

    const props = {
      items,
      totalCount: 2,
      onNavigate: vi.fn(),
      onClose: vi.fn(),
    };
    const { rerender } = render(<GridQuickLook {...props} currentIndex={0} />);
    const overlay = document.body.querySelector('[data-quick-look-overlay]');

    expect(overlay).toHaveAttribute('data-media-ready', 'true');
    expect(screen.getByTestId('group-content')).toBeInTheDocument();

    rerender(<GridQuickLook {...props} currentIndex={1} />);

    expect(document.body.querySelector('[data-quick-look-overlay]')).toBe(overlay);
    expect(overlay).toHaveAttribute('data-media-ready', 'true');
    expect(screen.getByTestId('group-content')).toBeInTheDocument();

    await act(async () => { resolveDecode?.(); });
    await waitFor(() => expect(screen.getByTestId('media-content')).toBeInTheDocument());
    expect(screen.queryByTestId('group-content')).not.toBeInTheDocument();
  });

  it('keeps the previous image mounted until the next image thumbnail is decoded', async () => {
    let resolveDecode: (() => void) | null = null;
    class ControlledImage {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      decode = () => new Promise<void>((resolve) => { resolveDecode = resolve; });
      set src(_value: string) { this.onload?.(); }
    }
    vi.stubGlobal('Image', ControlledImage);

    const props = {
      items,
      totalCount: 3,
      onNavigate: vi.fn(),
      onClose: vi.fn(),
    };
    const { rerender } = render(<GridQuickLook {...props} currentIndex={1} />);

    expect(screen.getByTestId('media-content')).toHaveAttribute('data-item-index', '1');
    rerender(<GridQuickLook {...props} currentIndex={2} />);
    expect(screen.getByTestId('media-content')).toHaveAttribute('data-item-index', '1');

    await act(async () => { resolveDecode?.(); });
    await waitFor(() => expect(screen.getByTestId('media-content')).toHaveAttribute('data-item-index', '2'));
  });
});
