import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ProgressiveMediaFrame } from './ProgressiveMediaFrame';

describe('ProgressiveMediaFrame', () => {
  it('keeps both layers mounted and changes only their visibility state', () => {
    const { container, rerender } = render(
      <ProgressiveMediaFrame
        preview={<div data-testid="preview" />}
        previewVisible
        contentReady={false}
      >
        <div data-testid="content" />
      </ProgressiveMediaFrame>,
    );

    expect(container.querySelector('[data-progressive-media-preview]')).toHaveAttribute('data-visible', 'true');
    expect(container.querySelector('[data-progressive-media-content]')).toHaveAttribute('data-visible', 'false');

    rerender(
      <ProgressiveMediaFrame
        preview={<div data-testid="preview" />}
        previewVisible={false}
        contentReady
      >
        <div data-testid="content" />
      </ProgressiveMediaFrame>,
    );

    expect(container.querySelector('[data-testid="preview"]')).toBeInTheDocument();
    expect(container.querySelector('[data-testid="content"]')).toBeInTheDocument();
    expect(container.querySelector('[data-progressive-media-preview]')).toHaveAttribute('data-visible', 'false');
    expect(container.querySelector('[data-progressive-media-content]')).toHaveAttribute('data-visible', 'true');
  });
});
