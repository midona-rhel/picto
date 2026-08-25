import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { LibraryAvatar } from './LibraryAvatar';

describe('LibraryAvatar', () => {
  it('fills its bounds with the selected image thumbnail', () => {
    render(<LibraryAvatar appearance={{ imageHash: 'abc123' }} size={26} />);

    const image = screen.getByRole('img', { hidden: true });
    expect(image).toHaveAttribute('src', 'media://localhost/thumb/abc123.jpg');
    expect(image.parentElement).toHaveStyle({ width: '26px', height: '26px' });
  });

  it('uses a lighter full-size default library glyph', () => {
    const { container } = render(<LibraryAvatar appearance={{}} size={26} />);
    const svg = container.querySelector('svg');

    expect(svg).toHaveAttribute('width', '26');
    expect(svg).toHaveAttribute('height', '26');
    expect(svg).toHaveAttribute('stroke-width', '1');
  });
});
