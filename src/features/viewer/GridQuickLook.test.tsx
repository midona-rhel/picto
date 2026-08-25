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
  QuickLookContent: () => <div data-testid="media-content" />,
}));

const items = [
  { item_id: 7, kind: 'collection', display_file_hash: 'group', display_mime_type: 'application/x-collection' },
  { item_id: 8, kind: 'media', display_file_hash: 'media', display_mime_type: 'image/jpeg' },
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
});
