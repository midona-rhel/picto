import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { DragGhost } from './DragGhost';

describe('DragGhost', () => {
  it('mattes transparent thumbnails before applying drag opacity', () => {
    render(<DragGhost
      x={100}
      y={100}
      count={1}
      thumbnailHashes={['transparent-image']}
      thumbnailBackgrounds={['#345678']}
    />);

    const thumbnail = document.querySelector('img[src*="transparent-image"]');
    expect(thumbnail).toHaveStyle({ background: '#345678' });
    expect(thumbnail?.parentElement?.parentElement).toHaveStyle({ opacity: '0.85' });
  });
});
