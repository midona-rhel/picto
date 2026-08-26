import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { QuickLook } from './QuickLook';

const detailRenderer = vi.fn();
const imagePipeline = vi.hoisted(() => ({
  displayedHash: null as string | null,
  thumbUrl: '',
  thumbLoaded: false,
  fullUrl: '',
  fullVisible: false,
  handleThumbLoad: vi.fn(),
  handleFullLoad: vi.fn(),
}));

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
  useMediaImagePipeline: () => imagePipeline,
}));

vi.mock('./hooks/useRecordMediaView', () => ({ useRecordMediaView: vi.fn() }));
vi.mock('./document/DetailMediaRenderer', () => ({
  DetailMediaRenderer: (props: { hash: string; mimeType: string; mediaKeyboardShortcutsEnabled?: boolean }) => {
    detailRenderer(props);
    return <div data-testid="detail-media-renderer" />;
  },
}));

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

  it('keeps the thumbnail and decoded image under one transform owner', () => {
    render(
      <QuickLook
        items={[{
          item_id: 1,
          display_file_hash: 'image-hash',
          display_mime_type: 'image/jpeg',
          pixel_width: 100,
          pixel_height: 100,
        } as never]}
        currentIndex={0}
        onNavigate={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    const overlay = document.body.querySelector('[data-quick-look-overlay]');
    expect(overlay?.querySelectorAll('[data-image-crossfade-frame]')).toHaveLength(1);
  });

  it('opens the shared media context menu from Quick Look', async () => {
    render(
      <QuickLook
        items={[{
          item_id: 1,
          kind: 'media',
          lifecycle: 'active',
          name: 'Image',
          display_file_hash: 'image-hash',
          display_mime_type: 'image/jpeg',
          pixel_width: 100,
          pixel_height: 100,
        } as never]}
        currentIndex={0}
        onNavigate={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    fireEvent.contextMenu(document.body.querySelector('[data-image-crossfade-frame]')!.parentElement!);
    fireEvent.click(await screen.findByRole('menuitem', { name: 'More' }));
    expect(await screen.findByRole('menuitem', { name: 'Set as Library Cover' })).toBeInTheDocument();
  });

  it('keeps a JPEG quick look transparent until its thumbnail is ready', async () => {
    imagePipeline.thumbLoaded = false;
    const props = {
      items: [{
        item_id: 1,
        display_file_hash: 'image-hash',
        display_mime_type: 'image/jpeg',
        pixel_width: 100,
        pixel_height: 100,
      } as never],
      currentIndex: 0,
      onNavigate: vi.fn(),
      onClose: vi.fn(),
    };
    const { rerender } = render(<QuickLook {...props} />);

    expect(document.body.querySelector('[data-quick-look-overlay]')).toHaveAttribute('data-media-ready', 'false');

    imagePipeline.thumbLoaded = true;
    rerender(<QuickLook {...props} />);
    await waitFor(() => expect(document.body.querySelector('[data-quick-look-overlay]')).toHaveAttribute('data-media-ready', 'true'));
    imagePipeline.thumbLoaded = false;
  });

  it.each([
    'application/pdf',
    'application/x-shockwave-flash',
    'font/ttf',
    'text/markdown',
    'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    'application/vnd.openxmlformats-officedocument.presentationml.presentation',
    'application/epub+zip',
    'application/vnd.comicbook+zip',
    'image/vnd.djvu',
    'image/jxl',
  ])('routes %s through the same canonical detail renderer', (mimeType) => {
    detailRenderer.mockClear();
    render(
      <QuickLook
        items={[{
          item_id: 1,
          display_file_hash: 'document-hash',
          display_mime_type: mimeType,
          name: 'Document',
        } as never]}
        currentIndex={0}
        onNavigate={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(detailRenderer).toHaveBeenCalledWith(expect.objectContaining({
      hash: 'document-hash',
      mimeType,
    }));
  });
});
