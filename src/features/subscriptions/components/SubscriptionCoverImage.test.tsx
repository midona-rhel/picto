import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import {
  SubscriptionCoverImage,
  SubscriptionCoverThumbnail,
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

  it('renders saved covers from one fixed thumbnail without runtime crop geometry', () => {
    render(<SubscriptionCoverThumbnail fileHash={'a'.repeat(64)} alt="Subscription cover" />);

    const image = screen.getByRole('img', { name: 'Subscription cover' });
    expect(image).toHaveAttribute('src', `media://localhost/thumb/${'a'.repeat(64)}.jpg`);
    expect(image).toHaveStyle({
      inset: '0',
      width: '100%',
      height: '100%',
      objectFit: 'cover',
    });
  });
});
