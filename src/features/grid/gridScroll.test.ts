import { describe, expect, it } from 'vitest';
import { scrollGridItemIntoView } from './gridScroll';

function scrollContainer(scrollTop: number, clientHeight = 400) {
  const container = document.createElement('div');
  Object.defineProperty(container, 'clientHeight', { value: clientHeight });
  container.scrollTop = scrollTop;
  return container;
}

const layout = {
  positions: [{ x: 0, y: 900, w: 180, h: 200 }],
  totalHeight: 2000,
};

describe('scrollGridItemIntoView', () => {
  it('centers an item for Quick Look navigation', () => {
    const container = scrollContainer(300);
    expect(scrollGridItemIntoView(container, layout, 0, 'center')).toBe(800);
  });

  it('includes subfolder header space when centering', () => {
    const container = scrollContainer(300);
    const layoutElement = document.createElement('div');
    layoutElement.dataset.gridLayout = '';
    Object.defineProperty(layoutElement, 'offsetTop', { value: 120 });
    container.appendChild(layoutElement);

    expect(scrollGridItemIntoView(container, layout, 0, 'center')).toBe(920);
  });

  it('does not move an already visible item with normal nearest alignment', () => {
    const container = scrollContainer(750);
    expect(scrollGridItemIntoView(container, layout, 0)).toBe(750);
  });
});
