import { useCallback, useEffect, useMemo, useState } from 'react';
import { Loader, Modal } from '@mantine/core';
import { useMantineColorScheme } from '@mantine/core';
import { glassModalStyles } from '../shared/styles/glassModal';
import Dagre from '@dagrejs/dagre';
import { api } from '#desktop/api';
import { getNamespaceColor } from '../shared/lib/namespaceColors';
import { useNavigationStore } from '../state/navigationStore';
import classes from './TagRelationsModal.module.css';

import type { TagRelation } from '../shared/types/api';

interface TagInfo {
  tag_id: number;
  namespace: string;
  subtag: string;
}

interface TagRelationsModalProps {
  opened: boolean;
  onClose: () => void;
  tag: TagInfo | null;
  source: 'local' | 'ptr';
}

type ViewMode = 'children' | 'siblings';

interface LayoutNode {
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
  label: string;
  ns: string;
  isCurrent: boolean;
}

interface LayoutEdge {
  from: string;
  to: string;
}

// ── Helpers ──

function formatTag(ns: string, st: string): string {
  return ns ? `${ns}:${st}` : st;
}

// ── Text measurement ──

let _canvas: HTMLCanvasElement | null = null;
function measureText(text: string): number {
  if (!_canvas) _canvas = document.createElement('canvas');
  const ctx = _canvas.getContext('2d')!;
  ctx.font = '13px -apple-system, BlinkMacSystemFont, sans-serif';
  return Math.ceil(ctx.measureText(text).width);
}

const NODE_H = 26;
const NODE_PAD = 32; // dot + horizontal padding

function nodeWidth(label: string): number {
  return Math.max(60, measureText(label) + NODE_PAD);
}

// ── Dagre layout ──

function computeLayout(
  rawNodes: { id: string; label: string; ns: string; isCurrent: boolean }[],
  rawEdges: LayoutEdge[],
): { nodes: LayoutNode[]; edges: LayoutEdge[]; width: number; height: number } {
  const g = new Dagre.graphlib.Graph().setDefaultEdgeLabel(() => ({}));
  g.setGraph({ rankdir: 'TB', nodesep: 12, ranksep: 36, marginx: 20, marginy: 20 });

  for (const n of rawNodes) {
    const w = nodeWidth(n.label);
    g.setNode(n.id, { width: w, height: NODE_H });
  }
  for (const e of rawEdges) {
    g.setEdge(e.from, e.to);
  }

  Dagre.layout(g);

  const graph = g.graph();
  const nodes: LayoutNode[] = rawNodes.map((n) => {
    const pos = g.node(n.id);
    const w = nodeWidth(n.label);
    return { id: n.id, x: pos.x - w / 2, y: pos.y - NODE_H / 2, w, h: NODE_H, label: n.label, ns: n.ns, isCurrent: n.isCurrent };
  });

  return { nodes, edges: rawEdges, width: graph.width ?? 200, height: graph.height ?? 100 };
}

// ── Build hierarchy graph ──
// parents at top → current → children at bottom

function buildHierarchy(parents: TagInfo[], current: TagInfo, childTags: TagInfo[]) {
  const rawNodes: { id: string; label: string; ns: string; isCurrent: boolean }[] = [];
  const rawEdges: LayoutEdge[] = [];

  for (const p of parents) {
    rawNodes.push({ id: `p-${p.tag_id}`, label: formatTag(p.namespace, p.subtag), ns: p.namespace, isCurrent: false });
  }
  rawNodes.push({ id: 'cur', label: formatTag(current.namespace, current.subtag), ns: current.namespace, isCurrent: true });
  for (const c of childTags) {
    rawNodes.push({ id: `c-${c.tag_id}`, label: formatTag(c.namespace, c.subtag), ns: c.namespace, isCurrent: false });
  }

  for (let i = 1; i < parents.length; i++) {
    rawEdges.push({ from: `p-${parents[i - 1].tag_id}`, to: `p-${parents[i].tag_id}` });
  }
  if (parents.length > 0) {
    rawEdges.push({ from: `p-${parents[parents.length - 1].tag_id}`, to: 'cur' });
  }
  for (const c of childTags) {
    rawEdges.push({ from: 'cur', to: `c-${c.tag_id}` });
  }

  return computeLayout(rawNodes, rawEdges);
}

// ── Build siblings graph ──
// ideal/superior at top → current → subordinates at bottom

function buildSiblings(current: TagInfo, siblings: TagRelation[]) {
  const rawNodes: { id: string; label: string; ns: string; isCurrent: boolean }[] = [];
  const rawEdges: LayoutEdge[] = [];

  const superiors = siblings.filter((s) => s.relation === 'to');
  const subordinates = siblings.filter((s) => s.relation === 'from');

  // Superior nodes first (ideal direction, goes at top)
  for (const sup of superiors) {
    rawNodes.push({ id: `s-${sup.tag_id}`, label: formatTag(sup.namespace, sup.subtag), ns: sup.namespace, isCurrent: false });
  }

  // Current node
  rawNodes.push({ id: 'cur', label: formatTag(current.namespace, current.subtag), ns: current.namespace, isCurrent: true });

  // Subordinate nodes (go at bottom)
  for (const sub of subordinates) {
    rawNodes.push({ id: `s-${sub.tag_id}`, label: formatTag(sub.namespace, sub.subtag), ns: sub.namespace, isCurrent: false });
  }

  // Edges: superiors → current (superior at top, current below)
  for (const sup of superiors) {
    rawEdges.push({ from: `s-${sup.tag_id}`, to: 'cur' });
  }

  // Edges: current → subordinates (current above, subordinates below)
  for (const sub of subordinates) {
    rawEdges.push({ from: 'cur', to: `s-${sub.tag_id}` });
  }

  return computeLayout(rawNodes, rawEdges);
}

// ── Rounded step edge path ──

function edgePath(from: LayoutNode, to: LayoutNode): string {
  const sx = from.x + from.w / 2;
  const sy = from.y + from.h;
  const tx = to.x + to.w / 2;
  const ty = to.y;
  const midY = (sy + ty) / 2;

  if (Math.abs(tx - sx) < 1) {
    return `M${sx},${sy} V${ty}`;
  }

  const maxR = 6;
  const r = Math.min(maxR, Math.abs(tx - sx) / 2, Math.abs(ty - sy) / 4);
  const goRight = tx > sx;
  const dx = goRight ? r : -r;

  // First bend (vertical→horizontal): same direction as turn
  // Second bend (horizontal→vertical): opposite sweep for smooth join
  const s1 = goRight ? 0 : 1;
  const s2 = goRight ? 1 : 0;

  return [
    `M${sx},${sy}`,
    `V${midY - r}`,
    `A${r},${r} 0 0,${s1} ${sx + dx},${midY}`,
    `H${tx - dx}`,
    `A${r},${r} 0 0,${s2} ${tx},${midY + r}`,
    `V${ty}`,
  ].join(' ');
}

// ── Chip colors ──

function chipColors(ns: string, isDark: boolean) {
  const [r, g, b] = getNamespaceColor(ns, isDark);
  return {
    bg: isDark ? `rgba(${r},${g},${b},0.12)` : `rgba(${r},${g},${b},0.10)`,
    border: `rgba(${r},${g},${b},0.25)`,
    activeBorder: `rgba(${r},${g},${b},0.7)`,
    dot: `rgb(${r},${g},${b})`,
    text: isDark ? 'rgba(255,255,255,0.85)' : `rgb(${r},${g},${b})`,
  };
}

// ── Component ──

export function TagRelationsModal({ opened, onClose, tag, source }: TagRelationsModalProps) {
  const [loading, setLoading] = useState(false);
  const [parents, setParents] = useState<TagInfo[]>([]);
  const [children, setChildren] = useState<TagInfo[]>([]);
  const [siblings, setSiblings] = useState<TagRelation[]>([]);
  const [view, setView] = useState<ViewMode>('children');
  const { colorScheme } = useMantineColorScheme();
  const isDark = colorScheme === 'dark';

  useEffect(() => {
    if (!opened || !tag) return;
    setLoading(true);
    setParents([]);
    setChildren([]);
    setSiblings([]);

    Promise.all([
      (source === 'ptr' ? api.ptr.getTagSiblings(tag.tag_id) : api.tags.getSiblings(tag.tag_id)).catch(() => []),
      (source === 'ptr' ? api.ptr.getTagParents(tag.tag_id) : api.tags.getParents(tag.tag_id)).catch(() => []),
    ]).then(([sibs, rels]) => {
      const p = rels.filter((r) => r.relation === 'parent');
      const c = rels.filter((r) => r.relation === 'child');
      setSiblings(sibs);
      setParents(p);
      setChildren(c);
      if (c.length > 0) setView('children');
      else if (sibs.length > 0) setView('siblings');
      setLoading(false);
    });
  }, [opened, tag, source]);

  const handleClickTag = useCallback(
    (label: string) => {
      useNavigationStore.getState().navigateToFilterTags([label]);
      onClose();
    },
    [onClose],
  );

  const hierarchy = useMemo(() => {
    if (!tag) return null;
    return buildHierarchy(parents, tag, children);
  }, [tag, parents, children]);

  const siblingGraph = useMemo(() => {
    if (!tag) return null;
    return buildSiblings(tag, siblings);
  }, [tag, siblings]);

  if (!tag) return null;

  const display = formatTag(tag.namespace, tag.subtag);
  const hasAny = parents.length > 0 || children.length > 0 || siblings.length > 0;
  const graph = view === 'children' ? hierarchy : siblingGraph;

  return (
    <Modal
      opened={opened}
      onClose={onClose}
      title={`Relations \u2014 ${display}`}
      centered
      size={!loading && !hasAny ? 'sm' : 'auto'}
      styles={glassModalStyles}
    >
      <div className={classes.root}>
        {loading ? (
          <div className={classes.loading}><Loader size="sm" /></div>
        ) : !hasAny ? (
          <div className={classes.empty}>No relations found for this tag.</div>
        ) : (
          <>
            <div className={classes.tabs}>
              <button
                className={`${classes.tab} ${view === 'children' ? classes.tabActive : ''}`}
                onClick={() => setView('children')}
              >
                Hierarchy
                {parents.length + children.length > 0 && (
                  <span className={classes.tabBadge}>{parents.length + children.length}</span>
                )}
              </button>
              <button
                className={`${classes.tab} ${view === 'siblings' ? classes.tabActive : ''}`}
                onClick={() => setView('siblings')}
              >
                Siblings
                {siblings.length > 0 && (
                  <span className={classes.tabBadge}>{siblings.length}</span>
                )}
              </button>
            </div>

            {graph && (
              <svg
                className={classes.svg}
                width={graph.width}
                height={graph.height}
                viewBox={`0 0 ${graph.width} ${graph.height}`}
              >
                {/* Edges */}
                {graph.edges.map((e, i) => {
                  const fromNode = graph.nodes.find((n) => n.id === e.from);
                  const toNode = graph.nodes.find((n) => n.id === e.to);
                  if (!fromNode || !toNode) return null;
                  return <path key={i} d={edgePath(fromNode, toNode)} className={classes.edge} />;
                })}

                {/* Nodes as chips */}
                {graph.nodes.map((n) => {
                  const c = chipColors(n.ns, isDark);
                  return (
                    <g key={n.id} onClick={() => handleClickTag(n.label)} style={{ cursor: 'pointer' }}>
                      <rect
                        x={n.x}
                        y={n.y}
                        width={n.w}
                        height={n.h}
                        rx={4}
                        ry={4}
                        fill={c.bg}
                        stroke={n.isCurrent ? c.activeBorder : c.border}
                        strokeWidth={n.isCurrent ? 1.5 : 1}
                      />
                      <circle cx={n.x + 11} cy={n.y + n.h / 2} r={3} fill={c.dot} />
                      <text
                        x={n.x + 21}
                        y={n.y + n.h / 2}
                        dominantBaseline="central"
                        fill={c.text}
                        fontSize={13}
                        fontFamily="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
                        fontWeight={n.isCurrent ? 600 : 400}
                      >
                        {n.label}
                      </text>
                    </g>
                  );
                })}
              </svg>
            )}
          </>
        )}
      </div>
    </Modal>
  );
}
