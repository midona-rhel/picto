import { createRef } from 'react';
import { render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ImageCrossfadeFrame } from './ImageCrossfadeFrame';

describe('ImageCrossfadeFrame', () => {
  it('keeps thumbnail and full-resolution layers in one positioned frame', () => {
    const frameRef = createRef<HTMLDivElement>();
    const fullImageRef = createRef<HTMLImageElement>();
    const { container } = render(<ImageCrossfadeFrame
      frameRef={frameRef}
      fullImageRef={fullImageRef}
      imageSize={{ width: 1200, height: 800 }}
      thumbnailUrl="thumb.jpg"
      fullUrl="full.jpg"
      thumbnailVisible
      fullVisible={false}
      imageRendering="pixelated"
      showTransparencyGrid
      onThumbnailLoad={vi.fn()}
      onFullLoad={vi.fn()}
    />);

    const frame = container.querySelector('.image-crossfade-frame');
    expect(frame).toHaveStyle({
      left: '50%',
      top: '50%',
      width: '1200px',
      height: '800px',
      aspectRatio: '1200 / 800',
      overflow: 'hidden',
    });
    expect(frame?.querySelectorAll('[data-image-crossfade-layer]')).toHaveLength(2);
    for (const layer of frame?.querySelectorAll('[data-image-crossfade-layer]') ?? []) {
      expect(layer).toHaveStyle({
        position: 'absolute',
        inset: '0',
        width: '100%',
        height: '100%',
        maxWidth: 'none',
        maxHeight: 'none',
        objectFit: 'fill',
        objectPosition: 'center',
        imageRendering: 'pixelated',
      });
    }
    expect(frame?.className).toContain('transparencyGrid');
  });

  it('does not expose intrinsic thumbnail geometry before full media dimensions are known', () => {
    const { container } = render(<ImageCrossfadeFrame
      frameRef={createRef<HTMLDivElement>()}
      fullImageRef={createRef<HTMLImageElement>()}
      imageSize={null}
      thumbnailUrl="thumb.jpg"
      fullUrl="full.jpg"
      thumbnailVisible
      fullVisible
      onThumbnailLoad={vi.fn()}
      onFullLoad={vi.fn()}
    />);

    expect(container.querySelector('[data-progressive-media-preview]')).toHaveAttribute('data-visible', 'false');
    expect(container.querySelector('[data-progressive-media-content]')).toHaveAttribute('data-visible', 'false');
  });

  it('replaces the ready thumbnail and its geometry in the same render', () => {
    const props = {
      frameRef: createRef<HTMLDivElement>(),
      fullImageRef: createRef<HTMLImageElement>(),
      imageSize: { width: 1200, height: 800 },
      fullUrl: '',
      thumbnailVisible: true,
      fullVisible: false,
      onThumbnailLoad: vi.fn(),
      onFullLoad: vi.fn(),
    };
    const { container, rerender } = render(
      <ImageCrossfadeFrame {...props} thumbnailUrl="first-thumb.jpg" />,
    );

    rerender(<ImageCrossfadeFrame
      {...props}
      imageSize={{ width: 800, height: 1200 }}
      thumbnailUrl="second-thumb.jpg"
    />);

    expect(container.querySelectorAll('[data-image-crossfade-layer="thumbnail"]')).toHaveLength(1);
    expect(container.querySelector('[data-image-crossfade-thumbnail="displayed"]')).toHaveAttribute(
      'src',
      'second-thumb.jpg',
    );
    expect(container.querySelector('.image-crossfade-frame')).toHaveStyle({
      width: '800px',
      height: '1200px',
      aspectRatio: '800 / 1200',
    });
  });
});
