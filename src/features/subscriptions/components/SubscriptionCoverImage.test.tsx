import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import {
  SubscriptionCoverImage,
  subscriptionCoverGeometry,
} from './SubscriptionCoverImage';

describe('SubscriptionCoverImage', () => {
  it('uses the original file and the shared crop geometry', () => {
    const crop = { focusX: 250, focusY: 750, zoomPercent: 160 };
    const geometry = subscriptionCoverGeometry({ width: 1200, height: 800 }, crop);
    render(
      <SubscriptionCoverImage
        fileHash="cover-hash"
        crop={crop}
        fallbackDimensions={{ width: 1200, height: 800 }}
        alt="Saved cover"
      />,
    );

    const image = screen.getByRole('img', { name: 'Saved cover' });
    expect(image).toHaveAttribute('src', 'media://localhost/file/cover-hash.bin');
    expect(image).toHaveStyle({
      width: `${geometry.widthRatio * 100}%`,
      height: `${geometry.heightRatio * 100}%`,
      left: `${geometry.leftPercent}%`,
      top: `${geometry.topPercent}%`,
    });
  });
});
