import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { LibraryAvatar } from './LibraryAvatar';

describe('LibraryAvatar', () => {
  it('fills its bounds with the selected image thumbnail', () => {
    const { container } = render(<LibraryAvatar appearance={{ imageHash: 'abc123' }} size={26} />);

    const image = screen.getByRole('img', { hidden: true });
    expect(image).toHaveAttribute('src', 'media://localhost/thumb/abc123.jpg');
    expect(image.parentElement).toHaveStyle({ width: '26px', height: '26px' });
    expect(container.firstElementChild?.className).not.toContain('undefined');
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

  it('resolves an inactive library cover from that library', () => {
    render(<LibraryAvatar appearance={{
      imageHash: 'abc123',
      libraryPath: '/Pictures/Archive.library',
    }} size={26} />);

    expect(screen.getByRole('img', { hidden: true })).toHaveAttribute(
      'src',
      'media://localhost/library-cover/cover?library=%2FPictures%2FArchive.library&v=abc123',
    );
  });

  it('changes the root cover URL when a different cover is selected', () => {
    const { rerender } = render(<LibraryAvatar appearance={{
      imageHash: 'first',
      libraryPath: '/Pictures/Main.library',
    }} size={26} />);
    expect(screen.getByRole('img', { hidden: true }).getAttribute('src')).toContain('&v=first');

    rerender(<LibraryAvatar appearance={{
      imageHash: 'second',
      libraryPath: '/Pictures/Main.library',
    }} size={26} />);

    expect(screen.getByRole('img', { hidden: true }).getAttribute('src')).toContain('&v=second');
  });

  it('uses the root cover when its global metadata was lost', () => {
    render(<LibraryAvatar appearance={{
      hasMaterializedCover: true,
      libraryPath: '/Pictures/Main.library',
    }} size={26} />);

    expect(screen.getByRole('img', { hidden: true })).toHaveAttribute(
      'src',
      'media://localhost/library-cover/cover?library=%2FPictures%2FMain.library',
    );
  });

  it('uses a lighter full-size default library glyph', () => {
    const { container } = render(<LibraryAvatar appearance={{}} size={26} />);
    const svg = container.querySelector('svg');

    expect(svg).toHaveAttribute('width', '19');
    expect(svg).toHaveAttribute('height', '19');
    expect(svg).toHaveAttribute('stroke-width', '0.9');
  });

  it('uses the circular highlighted treatment for the active library', () => {
    const { container } = render(<LibraryAvatar appearance={{}} size={39} highlighted />);
    const avatar = container.firstElementChild;

    expect(avatar?.className).toContain('highlighted');
    expect(avatar).toHaveStyle({ width: '39px', height: '39px' });
  });
});
