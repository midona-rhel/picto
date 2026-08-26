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
    const group = renderToStaticMarkup(<GroupIcon />);
    expect(group).toContain('data-picto-icon="group"');
    expect(group).toContain('width="10.5" height="14"');
    expect(group).toContain('stroke-width="1.25"');
    expect(group).toContain('M15.5 4v11.75');
    expect(group).not.toContain('M17.25');
    expect(renderToStaticMarkup(<GroupCreateIcon />)).toContain('data-picto-icon="group-create"');
    expect(renderToStaticMarkup(<GroupRemoveIcon />)).toContain('data-picto-icon="group-remove"');
    expect(renderToStaticMarkup(<GroupEditIcon />)).toContain('data-picto-icon="group-edit"');
    for (const [name, icon] of [
      ['create', <GroupCreateIcon />],
      ['remove', <GroupRemoveIcon />],
      ['edit', <GroupEditIcon />],
    ] as const) {
      const markup = renderToStaticMarkup(icon);
      expect(markup).toContain(`data-picto-sub-icon="${name}"`);
      expect(markup).toContain('M8.75 15.5H4.5');
      expect(markup).toContain('M15.5 4v3.5M8.75 17.5H6.5');
      expect(markup.match(/stroke-width="1.25"/g)?.length).toBeGreaterThanOrEqual(3);
    }
    expect(renderToStaticMarkup(<GroupCreateIcon />)).toContain('M11 14.75h7.5');
    expect(renderToStaticMarkup(<GroupRemoveIcon />)).toContain('m11.5 11.5 6.5 6.5');
    expect(renderToStaticMarkup(<GroupEditIcon />)).toContain('m11.25 18.75 1.15-4.05');
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
