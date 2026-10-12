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
  QuickLookContent: ({ items, currentIndex, thumbnailUrlOverride }: {
    items: Array<{ root_id: number }>;
    currentIndex: number;
    thumbnailUrlOverride?: string;
  }) => (
    <div
      data-testid="media-content"
      data-item-index={currentIndex}
      data-item-id={items[currentIndex]?.root_id}
      data-thumbnail={thumbnailUrlOverride ?? ''}
    />
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

  it('keeps drawing a reviewed item that left the list until the next one is decoded', async () => {
    let resolveDecode: (() => void) | null = null;
    class ControlledImage {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      decode = () => new Promise<void>((resolve) => { resolveDecode = resolve; });
      set src(_value: string) { this.onload?.(); }
    }
    vi.stubGlobal('Image', ControlledImage);
    const media = (items as unknown as Array<{ root_id: number }>).slice(1);
    const props = { totalCount: 2, onNavigate: vi.fn(), onClose: vi.fn() };
    const { rerender } = render(<GridQuickLook {...props} items={media as never} currentIndex={0} />);
    await act(async () => { resolveDecode?.(); });

    // Item 8 was rejected: it leaves the inbox list and index 0 is now item 9.
    rerender(<GridQuickLook {...props} items={media.slice(1) as never} currentIndex={0} />);
    const content = screen.getByTestId('media-content');
    expect(content).toHaveAttribute('data-item-id', '8');
    expect(content.getAttribute('data-thumbnail')).not.toContain('media-next');

    await act(async () => { resolveDecode?.(); });
    await waitFor(() => expect(screen.getByTestId('media-content')).toHaveAttribute('data-item-id', '9'));
  });
});
