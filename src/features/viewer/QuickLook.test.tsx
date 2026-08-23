import { render } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { QuickLook } from './QuickLook';

vi.mock('../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: ReactNode }) => children,
}));

vi.mock('./hooks/useImageZoom', () => ({
  useImageZoom: () => ({
    state: { scale: 1, tx: 0, ty: 0 },
    isDragging: false,
    handlers: { onMouseDown: vi.fn() },
    setState: vi.fn(),
    animateZoomTo: vi.fn(),
    fitToWindow: vi.fn(),
    fitActual: vi.fn(),
  }),
}));

vi.mock('../../shared/hooks/useMediaImagePipeline', () => ({
  useMediaImagePipeline: () => ({
    displayedHash: null,
    thumbUrl: '',
    thumbLoaded: false,
    fullUrl: '',
    fullVisible: false,
    handleThumbLoad: vi.fn(),
    handleFullLoad: vi.fn(),
  }),
}));

vi.mock('./hooks/useRecordMediaView', () => ({ useRecordMediaView: vi.fn() }));

describe('QuickLook', () => {
  it('renders at document level so layout containment cannot constrain it to the grid', () => {
    const { container } = render(
      <div data-contained-grid>
        <QuickLook
          items={[{
            entity_hash: 'hash',
            mime_type: 'image/jpeg',
            pixel_width: 100,
            pixel_height: 100,
          } as never]}
          currentIndex={0}
          onNavigate={vi.fn()}
          onClose={vi.fn()}
        />
      </div>,
    );

    expect(container.querySelector('[data-quick-look-overlay]')).toBeNull();
    const overlay = document.body.querySelector('[data-quick-look-overlay]');
    expect(overlay?.parentElement).toBe(document.body);
    expect(overlay?.querySelectorAll('[data-toolbar-glyph]')).toHaveLength(3);
  });
});
