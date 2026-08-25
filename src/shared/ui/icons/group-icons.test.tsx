import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import {
  DeselectAllIcon,
  GroupCreateIcon,
  GroupEditIcon,
  GroupIcon,
  GroupRemoveIcon,
  SelectAllIcon,
} from './group-icons';

describe('Picto group icon family', () => {
  it('keeps the group layers and action cutouts in one shared family', () => {
    expect(renderToStaticMarkup(<GroupIcon />)).toContain('data-picto-icon="group"');
    expect(renderToStaticMarkup(<GroupCreateIcon />)).toContain('data-picto-icon="group-create"');
    expect(renderToStaticMarkup(<GroupRemoveIcon />)).toContain('data-picto-icon="group-remove"');
    expect(renderToStaticMarkup(<GroupEditIcon />)).toContain('data-picto-icon="group-edit"');
  });

  it('uses four rounded tiles and reserves the strike for deselect all', () => {
    const select = renderToStaticMarkup(<SelectAllIcon />);
    const deselect = renderToStaticMarkup(<DeselectAllIcon />);

    expect(select.match(/<rect/g)).toHaveLength(4);
    expect(deselect.match(/<rect/g)).toHaveLength(4);
    expect(select).toContain('opacity="0.72"');
    expect(deselect).toContain('M2.5 2.5 17.5 17.5');
  });
});
