/**
 * Fixture Lab — minimal reference renderer for rebuilt slices.
 *
 * Renders fixture data as raw JSON panels so rebuilders can inspect exact
 * shapes, verify field presence, and compare against rebuilt component output.
 *
 * Usage:
 *   Mount this at a /lab route or render it standalone during development.
 *   Each section shows one fixture scenario. Rebuilt slice components will
 *   eventually be rendered alongside these panels for visual parity comparison.
 *
 * This is deliberately minimal. It is NOT a component library or storybook.
 * It exists so rebuilt slices have concrete data to render against.
 */

import { useState } from 'react';
import * as sidebarFixtures from './fixtures/sidebar';
import * as gridFixtures from './fixtures/grid';
import * as inspectorFixtures from './fixtures/inspector';

type Section = 'sidebar' | 'grid' | 'inspector';

function JsonPanel({ label, data }: { label: string; data: unknown }) {
  return (
    <div style={{ marginBottom: 16 }}>
      <h3 style={{ margin: '0 0 4px', fontSize: 14, fontWeight: 600 }}>{label}</h3>
      <pre
        style={{
          background: '#1e1e2e',
          color: '#cdd6f4',
          padding: 12,
          borderRadius: 6,
          fontSize: 12,
          lineHeight: 1.4,
          overflow: 'auto',
          maxHeight: 400,
          margin: 0,
        }}
      >
        {JSON.stringify(data, null, 2)}
      </pre>
    </div>
  );
}

function SidebarSection() {
  return (
    <>
      <JsonPanel label="Standard sidebar tree (12 nodes, nested folders, smart folders)" data={sidebarFixtures.sidebarTreeStandard} />
      <JsonPanel label="Empty library sidebar (system scopes only, all zero)" data={sidebarFixtures.sidebarTreeEmpty} />
      <JsonPanel label="Stale sidebar (post-import, counts rebuilding)" data={sidebarFixtures.sidebarTreeStale} />
    </>
  );
}

function GridSection() {
  return (
    <>
      <JsonPanel label="Mixed grid page (images, videos, collections, 7 items)" data={gridFixtures.gridPageMixed} />
      <JsonPanel label="Empty grid page (no results)" data={gridFixtures.gridPageEmpty} />
      <JsonPanel label="Single item result" data={gridFixtures.gridPageSingle} />
      <JsonPanel label="Last page (no next_cursor)" data={gridFixtures.gridPageLast} />
      <JsonPanel label="Inbox items (status=0)" data={gridFixtures.gridPageInbox} />
      <h3 style={{ margin: '16px 0 4px', fontSize: 14, fontWeight: 600 }}>Matching queries</h3>
      <JsonPanel label="All active" data={gridFixtures.queryAllActive} />
      <JsonPanel label="Folder scope" data={gridFixtures.queryFolder} />
      <JsonPanel label="With filters (rating≥3, images only, tag=landscape)" data={gridFixtures.queryWithFilters} />
      <JsonPanel label="Search scope" data={gridFixtures.querySearch} />
    </>
  );
}

function InspectorSection() {
  return (
    <>
      <h3 style={{ margin: '0 0 4px', fontSize: 14, fontWeight: 600 }}>Entity details</h3>
      <JsonPanel label="Rich image (tags, folders, notes, rating, source URLs)" data={inspectorFixtures.detailsRichImage} />
      <JsonPanel label="Sparse image (no tags, no folders, no notes)" data={inspectorFixtures.detailsSparseImage} />
      <JsonPanel label="Video (duration, frame count)" data={inspectorFixtures.detailsVideo} />
      <JsonPanel label="Collection (47 members, total_size_bytes)" data={inspectorFixtures.detailsCollection} />
      <JsonPanel label="Inbox item (status=0, no thumbnail)" data={inspectorFixtures.detailsInboxItem} />
      <h3 style={{ margin: '16px 0 4px', fontSize: 14, fontWeight: 600 }}>Selection summaries</h3>
      <JsonPanel label="Single selection" data={inspectorFixtures.selectionSingle} />
      <JsonPanel label="Multi selection (3 items)" data={inspectorFixtures.selectionMulti} />
      <JsonPanel label="Virtual select-all (1247 items, no hashes)" data={inspectorFixtures.selectionVirtualAll} />
      <JsonPanel label="Empty selection" data={inspectorFixtures.selectionEmpty} />
    </>
  );
}

export function FixtureLab() {
  const [section, setSection] = useState<Section>('sidebar');

  const sections: { key: Section; label: string }[] = [
    { key: 'sidebar', label: 'Shell / Sidebar' },
    { key: 'grid', label: 'Grid' },
    { key: 'inspector', label: 'Inspector / Selection' },
  ];

  return (
    <div style={{ fontFamily: 'system-ui, sans-serif', padding: 24, maxWidth: 900 }}>
      <h1 style={{ fontSize: 20, margin: '0 0 16px' }}>Fixture Lab — Parity Reference</h1>
      <p style={{ fontSize: 13, color: '#888', margin: '0 0 16px' }}>
        Canonical fixture data for rebuilt slice parity checks. Rebuilt components
        should render these fixtures and match the legacy behavior documented in
        the parity checklists.
      </p>

      <div style={{ display: 'flex', gap: 8, marginBottom: 20 }}>
        {sections.map((s) => (
          <button
            key={s.key}
            onClick={() => setSection(s.key)}
            style={{
              padding: '6px 14px',
              borderRadius: 4,
              border: 'none',
              background: section === s.key ? '#7c3aed' : '#333',
              color: '#fff',
              cursor: 'pointer',
              fontSize: 13,
            }}
          >
            {s.label}
          </button>
        ))}
      </div>

      {section === 'sidebar' && <SidebarSection />}
      {section === 'grid' && <GridSection />}
      {section === 'inspector' && <InspectorSection />}
    </div>
  );
}
