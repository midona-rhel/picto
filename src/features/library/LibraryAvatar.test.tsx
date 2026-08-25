import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { LibraryAvatar } from './LibraryAvatar';

describe('LibraryAvatar', () => {
  it('fills its bounds with the selected image thumbnail', () => {
    const { container } = render(<LibraryAvatar appearance={{ imageHash: 'abc123' }} size={26} />);

    const image = screen.getByRole('img', { hidden: true });
    expect(image).toHaveAttribute('src', 'media://localhost/thumb/abc123.jpg');
    expect(image.parentElement).toHaveStyle({ width: '26px', height: '26px' });
    expect(container.firstElementChild?.className).toContain('imageBacked');
  });

  it('renders the persisted crop rather than a generic center crop', () => {
    render(<LibraryAvatar appearance={{
      imageHash: 'abc123',
      imageFocusX: 250,
      imageFocusY: 750,
      imageZoomPercent: 150,
    }} size={56} />);

    const image = screen.getByRole('img', { hidden: true });
    expect(image).toHaveStyle({ width: '150%', height: '150%' });
    expect(image.style.left).not.toBe('50%');
    expect(image.style.top).not.toBe('50%');
  });

  it('uses a lighter full-size default library glyph', () => {
    const { container } = render(<LibraryAvatar appearance={{}} size={26} />);
    const svg = container.querySelector('svg');

    expect(svg).toHaveAttribute('width', '26');
    expect(svg).toHaveAttribute('height', '26');
    expect(svg).toHaveAttribute('stroke-width', '0.75');
  });
});
