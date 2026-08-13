import { describe, expect, it } from 'vitest';
import {
  buildDomExpression,
  buildDomTextContainerExpression,
  createPng,
  parseArgs,
  sidebarNode,
} from './duplicate-review-smoke.mjs';

describe('duplicate review smoke helpers', () => {
  it('creates distinct PNG files with identical pixels for duplicate seeding', () => {
    const first = createPng({ marker: 'first' });
    const second = createPng({ marker: 'second' });
    expect(first.subarray(0, 8)).toEqual(second.subarray(0, 8));
    expect(first.equals(second)).toBe(false);
  });

  it('parses smoke options and finds contract nodes', () => {
    expect(parseArgs(['--platform', 'darwin', '--timeout', '1000'])).toEqual({ platform: 'darwin', timeout: '1000' });
    const tree = { nodes: [{ id: 'system:active', count: 4 }] };
    expect(sidebarNode(tree, 'system:active')).toEqual({ id: 'system:active', count: 4 });
    expect(sidebarNode(tree, 'system:inbox')).toBeNull();
  });

  it('builds accessible role/text click expressions without another browser framework', () => {
    const roleExpression = buildDomExpression('role-click', 'Re-scan library');
    const textExpression = buildDomExpression('text-click', 'Duplicates');
    const containerExpression = buildDomTextContainerExpression('Duplicates');

    expect(roleExpression).toContain('button,[role="button"],[role="region"]');
    expect(roleExpression).toContain('aria-label');
    expect(roleExpression).toContain('candidate.getAttribute');
    expect(roleExpression).not.toContain("element.getAttribute('aria-label')");
    expect(roleExpression).toContain('.click()');
    expect(textExpression).toContain("querySelectorAll('*')");
    expect(textExpression).toContain('textContent?.trim() === target');
    expect(containerExpression).toContain('parentElement?.textContent');
  });
});
