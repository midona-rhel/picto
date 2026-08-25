import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ThumbnailImage } from './ThumbnailImage';

describe('ThumbnailImage', () => {
  it('replaces a failed thumbnail with Picto broken-file artwork', () => {
    const { container } = render(<ThumbnailImage src="media://localhost/thumb/missing.jpg" alt="" />);

    fireEvent.error(container.querySelector('img')!);

    expect(container.querySelector('[data-broken-thumbnail]')).not.toBeNull();
  });

  it('does not leak a native broken-image glyph for folder covers', () => {
    const { container } = render(<ThumbnailImage src="media://localhost/thumb/missing.jpg" fallback="empty" alt="" />);

    fireEvent.error(container.querySelector('img')!);

    expect(container.querySelector('img')).toBeNull();
  });

  it('renders fonts directly without requesting a raster thumbnail', () => {
    const { container } = render(<ThumbnailImage src="media://localhost/thumb/font.jpg" fallback="font" alt="" />);

    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('[data-font-thumbnail]')).not.toBeNull();
  });
});
